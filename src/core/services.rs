use std::{ffi::c_void, fs, io::ErrorKind, mem::size_of, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use windows::{
    Win32::{
        Foundation::ERROR_MORE_DATA,
        System::Services::{
            ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle,
            ENUM_SERVICE_STATUS_PROCESSW, ENUM_SERVICE_TYPE, EnumServicesStatusExW, OpenSCManagerW,
            OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfig2W, QueryServiceConfigW,
            SC_ENUM_PROCESS_INFO, SC_HANDLE, SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE,
            SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_ERROR,
            SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG, SERVICE_START_TYPE, SERVICE_STATE_ALL,
            SERVICE_WIN32,
        },
    },
    core::{BOOL, HRESULT, PCWSTR, PWSTR},
};

use crate::{config, core::journal::Journal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEntry {
    pub name: String,
    pub title: String,
    pub command: String,
    pub status: String,
    pub start_label: String,
    pub enabled: bool,
    pub system: bool,
    pub managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisabledService {
    pub name: String,
    pub display_name: String,
    pub binary_path: String,
    pub original_start_type: u32,
    pub delayed: bool,
    pub disabled_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DisabledServiceStore {
    entries: Vec<DisabledService>,
}

impl DisabledServiceStore {
    fn path() -> std::path::PathBuf {
        config::disabled_services_path()
    }
}

#[derive(Debug, Clone)]
struct ServiceConfig {
    display_name: String,
    binary_path: String,
    start_type: u32,
    delayed: bool,
}

pub fn scan_services() -> Result<Vec<ServiceEntry>> {
    let scm = open_scm()?;
    let store = load_store(&DisabledServiceStore::path())?;
    let mut entries = Vec::new();
    let mut seen_names = Vec::new();

    for (name, display_name, state) in enumerate_services(&scm)? {
        let Ok(service) = open_service(&scm, &name, SERVICE_QUERY_CONFIG) else {
            continue;
        };
        let Ok(config) = query_config(service.0) else {
            continue;
        };

        let disabled = config.start_type == SERVICE_DISABLED.0;
        let managed = disabled
            && store
                .entries
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case(&name));
        seen_names.push(name.clone());
        entries.push(ServiceEntry {
            title: if display_name.trim().is_empty() {
                if config.display_name.trim().is_empty() {
                    name.clone()
                } else {
                    config.display_name.clone()
                }
            } else {
                display_name
            },
            name,
            command: config.binary_path.clone(),
            status: if disabled {
                if managed {
                    "Отключена ChipClean"
                } else {
                    "Отключена вне ChipClean"
                }
            } else {
                service_state_label(state)
            }
            .to_owned(),
            start_label: start_type_label(config.start_type, config.delayed).to_owned(),
            enabled: !disabled,
            system: is_system_path(&config.binary_path),
            managed,
        });
    }

    for disabled in store.entries {
        if seen_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&disabled.name))
        {
            continue;
        }
        entries.push(ServiceEntry {
            title: disabled.display_name.clone(),
            name: disabled.name.clone(),
            command: disabled.binary_path.clone(),
            status: "Отключена ChipClean".to_owned(),
            start_label: start_type_label(disabled.original_start_type, disabled.delayed)
                .to_owned(),
            enabled: false,
            system: is_system_path(&disabled.binary_path),
            managed: true,
        });
    }

    entries.sort_by(|left, right| {
        left.system
            .cmp(&right.system)
            .then_with(|| right.enabled.cmp(&left.enabled))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    Ok(entries)
}

pub fn toggle_service(entry: &ServiceEntry) -> Result<String> {
    if entry.enabled {
        disable_service(entry)
    } else if entry.managed {
        enable_service(entry)
    } else {
        restore_manual_start(entry)
    }
}

fn disable_service(entry: &ServiceEntry) -> Result<String> {
    let scm = open_scm()?;
    let service = open_service(
        &scm,
        &entry.name,
        SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG,
    )?;
    let config = query_config(service.0)?;
    if config.start_type == SERVICE_DISABLED.0 {
        bail!("Служба уже отключена; обновите список.");
    }

    let store_path = DisabledServiceStore::path();
    let mut store = load_store(&store_path)?;
    store
        .entries
        .retain(|disabled| !disabled.name.eq_ignore_ascii_case(&entry.name));

    let disabled = DisabledService {
        name: entry.name.clone(),
        display_name: entry.title.clone(),
        binary_path: config.binary_path.clone(),
        original_start_type: config.start_type,
        delayed: config.delayed,
        disabled_at: Utc::now().to_rfc3339(),
    };
    store.entries.push(disabled.clone());
    change_start_type(service.0, SERVICE_DISABLED, false)?;

    if let Err(err) = save_store(&store_path, &store) {
        let _ = change_start_type(
            service.0,
            SERVICE_START_TYPE(config.start_type),
            config.delayed,
        );
        return Err(err.context("не удалось сохранить состояние; тип запуска восстановлен"));
    }

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "service_startup_disabled",
        "name": entry.name,
        "display": entry.title,
        "original_start_type": config.start_type,
        "delayed": config.delayed,
    }));

    let running = entry.status == "Работает";
    Ok(if running {
        "Автозапуск службы отключён. Служба продолжит работать до остановки или перезагрузки."
            .to_owned()
    } else {
        "Автозапуск службы отключён.".to_owned()
    })
}

fn enable_service(entry: &ServiceEntry) -> Result<String> {
    let store_path = DisabledServiceStore::path();
    let mut store = load_store(&store_path)?;
    let position = store
        .entries
        .iter()
        .position(|disabled| disabled.name.eq_ignore_ascii_case(&entry.name))
        .ok_or_else(|| anyhow!("Служба не найдена в состоянии ChipClean; обновите список."))?;
    let original = store.entries[position].clone();

    let scm = open_scm()?;
    let service = open_service(
        &scm,
        &original.name,
        SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG,
    )?;
    change_start_type(
        service.0,
        SERVICE_START_TYPE(original.original_start_type),
        original.delayed,
    )?;

    store.entries.remove(position);
    if let Err(err) = save_store(&store_path, &store) {
        let _ = change_start_type(service.0, SERVICE_DISABLED, false);
        return Err(err.context("не удалось сохранить состояние; служба снова отключена"));
    }

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "service_startup_enabled",
        "name": original.name,
        "display": original.display_name,
    }));

    Ok("Автозапуск службы восстановлен.".to_owned())
}

fn restore_manual_start(entry: &ServiceEntry) -> Result<String> {
    let scm = open_scm()?;
    let service = open_service(
        &scm,
        &entry.name,
        SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG,
    )?;
    let config = query_config(service.0)?;
    if config.start_type != SERVICE_DISABLED.0 {
        bail!("Служба уже не отключена; обновите список.");
    }

    change_start_type(service.0, SERVICE_DEMAND_START, false)?;

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "service_restored_manual",
        "name": entry.name,
        "display": entry.title,
        "previous_start_type": config.start_type,
    }));

    Ok("Служба переведена в ручной запуск. Автозапуск не включён.".to_owned())
}

fn open_scm() -> Result<ScmHandle> {
    let handle = unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
    }
    .context("не удалось открыть менеджер служб")?;
    Ok(ScmHandle(handle))
}

fn open_service(scm: &ScmHandle, name: &str, access: u32) -> Result<ServiceHandle> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe { OpenServiceW(scm.0, PCWSTR(wide.as_ptr()), access) }
        .with_context(|| format!("не удалось открыть службу {name}"))?;
    Ok(ServiceHandle(handle))
}

fn enumerate_services(scm: &ScmHandle) -> Result<Vec<(String, String, u32)>> {
    const MAX_BUFFER: usize = 64 * 1024 * 1024;
    let mut buffer_bytes: usize = 256 * 1024;
    let mut resume = 0_u32;
    let mut services = Vec::new();

    loop {
        let mut buffer = vec![0_u64; buffer_bytes.div_ceil(8)];
        let mut needed = 0_u32;
        let mut returned = 0_u32;
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 8)
        };

        let result = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(bytes),
                &mut needed,
                &mut returned,
                Some(&mut resume),
                PCWSTR::null(),
            )
        };

        if let Err(err) = result {
            if err.code() != HRESULT::from_win32(ERROR_MORE_DATA.0) {
                return Err(err).context("не удалось перечислить службы");
            }
            buffer_bytes = next_enum_buffer_size(buffer_bytes, needed);
            if buffer_bytes > MAX_BUFFER {
                bail!("перечисление служб требует буфер больше 64 МиБ");
            }
            continue;
        }

        let items = buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW;
        for index in 0..returned as usize {
            let item = unsafe { &*items.add(index) };
            let name = pwstr_to_string(item.lpServiceName);
            if name.is_empty() {
                continue;
            }
            services.push((
                name,
                pwstr_to_string(item.lpDisplayName),
                item.ServiceStatusProcess.dwCurrentState.0,
            ));
        }

        if resume == 0 {
            return Ok(services);
        }
    }
}

fn next_enum_buffer_size(current: usize, needed: u32) -> usize {
    let required = needed as usize;
    if required > current {
        required
    } else {
        current.saturating_mul(2)
    }
}

fn query_config(service: SC_HANDLE) -> Result<ServiceConfig> {
    let mut needed = 0_u32;
    let _ = unsafe { QueryServiceConfigW(service, None, 0, &mut needed) };
    let size = (needed as usize).max(1024);
    let mut buffer = vec![0_u64; size.div_ceil(8) + 8];
    let mut needed_again = 0_u32;
    unsafe {
        QueryServiceConfigW(
            service,
            Some(buffer.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
            (buffer.len() * 8) as u32,
            &mut needed_again,
        )
    }
    .context("не удалось прочитать конфигурацию службы")?;

    let config = unsafe { &*(buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
    let start_type = config.dwStartType.0;
    let delayed = if start_type == SERVICE_AUTO_START.0 {
        query_delayed(service).unwrap_or(false)
    } else {
        false
    };

    Ok(ServiceConfig {
        display_name: pwstr_to_string(config.lpDisplayName),
        binary_path: pwstr_to_string(config.lpBinaryPathName),
        start_type,
        delayed,
    })
}

fn query_delayed(service: SC_HANDLE) -> Result<bool> {
    let mut info = SERVICE_DELAYED_AUTO_START_INFO::default();
    let mut needed = 0_u32;
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(
            &mut info as *mut SERVICE_DELAYED_AUTO_START_INFO as *mut u8,
            size_of::<SERVICE_DELAYED_AUTO_START_INFO>(),
        )
    };
    unsafe {
        QueryServiceConfig2W(
            service,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            Some(bytes),
            &mut needed,
        )
    }
    .context("не удалось прочитать задержку запуска службы")?;
    Ok(info.fDelayedAutostart.as_bool())
}

fn change_start_type(
    service: SC_HANDLE,
    start_type: SERVICE_START_TYPE,
    delayed: bool,
) -> Result<()> {
    unsafe {
        ChangeServiceConfigW(
            service,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            start_type,
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    }
    .context("не удалось изменить тип запуска службы")?;

    if start_type == SERVICE_AUTO_START {
        let info = SERVICE_DELAYED_AUTO_START_INFO {
            fDelayedAutostart: BOOL::from(delayed),
        };
        unsafe {
            ChangeServiceConfig2W(
                service,
                SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                Some(&info as *const SERVICE_DELAYED_AUTO_START_INFO as *const c_void),
            )
        }
        .context("не удалось изменить задержку запуска службы")?;
    }

    Ok(())
}

fn service_state_label(state: u32) -> &'static str {
    match state {
        4 => "Работает",
        3 => "Останавливается",
        2 => "Запускается",
        7 => "Приостановлена",
        6 => "Приостанавливается",
        5 => "Продолжается",
        _ => "Остановлена",
    }
}

fn start_type_label(start_type: u32, delayed: bool) -> &'static str {
    if start_type == SERVICE_AUTO_START.0 {
        if delayed {
            "Авто (задержка)"
        } else {
            "Авто"
        }
    } else if start_type == SERVICE_DISABLED.0 {
        "Отключена"
    } else {
        "Вручную"
    }
}

fn is_system_path(path: &str) -> bool {
    let lower = path.trim_matches('"').to_ascii_lowercase();
    let root = std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".to_owned())
        .to_ascii_lowercase();
    lower.starts_with(&root)
}

fn pwstr_to_string(value: PWSTR) -> String {
    if value.0.is_null() {
        return String::new();
    }
    unsafe { value.to_string() }.unwrap_or_default()
}

fn load_store(path: &Path) -> Result<DisabledServiceStore> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(DisabledServiceStore::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось прочитать {}", path.display()));
        }
    };
    serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("не удалось разобрать {}", path.display()))
}

fn save_store(path: &Path, store: &DisabledServiceStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("не удалось создать {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(store)?;
    fs::write(path, raw).with_context(|| format!("не удалось записать {}", path.display()))
}

struct ScmHandle(SC_HANDLE);

impl Drop for ScmHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseServiceHandle(self.0) };
    }
}

struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseServiceHandle(self.0) };
    }
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write service journal event");
            } else {
                let _ = journal.flush();
            }
        }
        Err(err) => tracing::warn!(%err, "failed to open operations journal"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Services::{SERVICE_RUNNING, SERVICE_STOPPED};

    #[test]
    fn store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disabled-services.json");
        let mut store = DisabledServiceStore::default();
        store.entries.push(DisabledService {
            name: "TestService".to_owned(),
            display_name: "Test Service".to_owned(),
            binary_path: r"C:\Test\test.exe".to_owned(),
            original_start_type: SERVICE_AUTO_START.0,
            delayed: false,
            disabled_at: Utc::now().to_rfc3339(),
        });
        save_store(&path, &store).unwrap();
        let loaded = load_store(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].name, "TestService");
    }

    #[test]
    fn labels_are_human_readable() {
        assert_eq!(start_type_label(SERVICE_AUTO_START.0, false), "Авто");
        assert_eq!(
            start_type_label(SERVICE_AUTO_START.0, true),
            "Авто (задержка)"
        );
        assert_eq!(start_type_label(SERVICE_DISABLED.0, false), "Отключена");
        assert_eq!(start_type_label(3, false), "Вручную");
        assert_eq!(service_state_label(SERVICE_RUNNING.0), "Работает");
        assert_eq!(service_state_label(SERVICE_STOPPED.0), "Остановлена");
    }

    #[test]
    fn enumeration_buffer_grows_on_more_data() {
        assert_eq!(next_enum_buffer_size(256 * 1024, 300 * 1024), 300 * 1024);
        assert_eq!(next_enum_buffer_size(256 * 1024, 0), 512 * 1024);
        assert_eq!(next_enum_buffer_size(256 * 1024, 1024), 512 * 1024);
        assert_eq!(next_enum_buffer_size(4, 4), 8);
    }

    #[test]
    fn system_paths_are_detected() {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        assert!(is_system_path(&format!(r"{root}\System32\svchost.exe")));
        assert!(!is_system_path(r"C:\Program Files\Vendor\service.exe"));
    }
}
