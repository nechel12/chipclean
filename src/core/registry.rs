use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use crossbeam_channel::Sender;
use egui_phosphor::regular as ph;
use serde_json::json;
use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};

use super::{journal::Journal, model::Risk};
use crate::config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RegistryCategoryId {
    Uninstall,
    Startup,
    AppPaths,
}

impl RegistryCategoryId {
    pub fn key(self) -> &'static str {
        match self {
            Self::Uninstall => "registry_uninstall",
            Self::Startup => "registry_startup",
            Self::AppPaths => "registry_app_paths",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegistryCategory {
    pub id: RegistryCategoryId,
    pub title: &'static str,
    pub summary: &'static str,
    pub icon: &'static str,
    pub risk: Risk,
    pub default_enabled: bool,
}

pub fn catalog() -> Vec<RegistryCategory> {
    vec![
        RegistryCategory {
            id: RegistryCategoryId::Uninstall,
            title: "Неверные записи деинсталляции",
            summary: "Осиротевшие ключи Uninstall",
            icon: ph::PACKAGE,
            risk: Risk::Caution,
            default_enabled: true,
        },
        RegistryCategory {
            id: RegistryCategoryId::Startup,
            title: "Неверные записи автозагрузки",
            summary: "Run и RunOnce с отсутствующими файлами",
            icon: ph::ROCKET_LAUNCH,
            risk: Risk::Caution,
            default_enabled: true,
        },
        RegistryCategory {
            id: RegistryCategoryId::AppPaths,
            title: "Неверные пути App Paths",
            summary: "Регистрация программ с отсутствующим exe",
            icon: ph::APP_WINDOW,
            risk: Risk::Caution,
            default_enabled: true,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryHive {
    CurrentUser,
    LocalMachine,
}

impl RegistryHive {
    pub fn short(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKCU",
            Self::LocalMachine => "HKLM",
        }
    }

    pub(crate) fn reg_name(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKEY_CURRENT_USER",
            Self::LocalMachine => "HKEY_LOCAL_MACHINE",
        }
    }

    pub(crate) fn root(self) -> &'static Key {
        match self {
            Self::CurrentUser => CURRENT_USER,
            Self::LocalMachine => LOCAL_MACHINE,
        }
    }
}

#[derive(Debug, Clone)]
pub enum RegistryAction {
    DeleteValue {
        key_path: String,
        value_name: String,
    },
    DeleteKey {
        parent_path: String,
        subkey_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct RegistryFinding {
    pub category: RegistryCategoryId,
    pub hive: RegistryHive,
    pub title: String,
    pub detail: String,
    pub key_display: String,
    pub action: RegistryAction,
}

impl RegistryFinding {
    pub fn backup_target(&self) -> (RegistryHive, String) {
        match &self.action {
            RegistryAction::DeleteValue { key_path, .. } => (self.hive, key_path.clone()),
            RegistryAction::DeleteKey {
                parent_path,
                subkey_name,
            } => (self.hive, format!("{parent_path}\\{subkey_name}")),
        }
    }

    pub fn value_name(&self) -> Option<&str> {
        match &self.action {
            RegistryAction::DeleteValue { value_name, .. } => Some(value_name),
            RegistryAction::DeleteKey { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum RegistryScanEvent {
    Started {
        total: usize,
    },
    CategoryStarted {
        id: RegistryCategoryId,
    },
    CategoryFinished {
        findings: Vec<RegistryFinding>,
    },
    Finished {
        total_findings: usize,
        elapsed: Duration,
        cancelled: bool,
    },
    Failed(String),
}

pub fn spawn_scan(
    categories: Vec<RegistryCategoryId>,
    cancel: Arc<AtomicBool>,
    tx: Sender<RegistryScanEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("chipclean-registry-scan".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let total = categories.len();
            let mut total_findings = 0_usize;

            let _ = tx.send(RegistryScanEvent::Started { total });

            for id in categories {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }

                if tx.send(RegistryScanEvent::CategoryStarted { id }).is_err() {
                    return;
                }

                match scan_category(id, &cancel) {
                    Ok(findings) => {
                        total_findings += findings.len();
                        if tx
                            .send(RegistryScanEvent::CategoryFinished { findings })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(RegistryScanEvent::Failed(err.to_string()));
                        return;
                    }
                }
            }

            let _ = tx.send(RegistryScanEvent::Finished {
                total_findings,
                elapsed: started.elapsed(),
                cancelled: cancel.load(Ordering::Relaxed),
            });
        })
        .expect("не удалось создать поток проверки реестра")
}

fn scan_category(id: RegistryCategoryId, cancel: &AtomicBool) -> Result<Vec<RegistryFinding>> {
    match id {
        RegistryCategoryId::Uninstall => Ok(scan_uninstall(cancel)),
        RegistryCategoryId::Startup => Ok(scan_startup(cancel)),
        RegistryCategoryId::AppPaths => Ok(scan_app_paths(cancel)),
    }
}

pub(crate) const STARTUP_KEYS: &[(RegistryHive, &str)] = &[
    (
        RegistryHive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
    ),
    (
        RegistryHive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\RunOnce",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce",
    ),
];

const UNINSTALL_KEYS: &[(RegistryHive, &str)] = &[
    (
        RegistryHive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
];

const APP_PATH_KEYS: &[(RegistryHive, &str)] = &[
    (
        RegistryHive::CurrentUser,
        r"Software\Microsoft\Windows\CurrentVersion\App Paths",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
    ),
    (
        RegistryHive::LocalMachine,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths",
    ),
];

fn scan_startup(cancel: &AtomicBool) -> Vec<RegistryFinding> {
    let mut findings = Vec::new();

    for (hive, key_path) in STARTUP_KEYS {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let Ok(key) = hive.root().open(*key_path) else {
            continue;
        };
        let Ok(values) = key.values() else {
            continue;
        };

        for (name, _) in values {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let Ok(command) = key.get_string(&name) else {
                continue;
            };
            let Some(executable) = executable_path(&command) else {
                continue;
            };
            if !file_is_missing(&executable) {
                continue;
            }

            findings.push(RegistryFinding {
                category: RegistryCategoryId::Startup,
                hive: *hive,
                title: name.clone(),
                detail: format!("Не найден файл: {}", executable.display()),
                key_display: format!("{}\\{}", hive.short(), key_path),
                action: RegistryAction::DeleteValue {
                    key_path: (*key_path).to_owned(),
                    value_name: name,
                },
            });
        }
    }

    findings
}

fn scan_uninstall(cancel: &AtomicBool) -> Vec<RegistryFinding> {
    let mut findings = Vec::new();

    for (hive, parent_path) in UNINSTALL_KEYS {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let Ok(parent) = hive.root().open(*parent_path) else {
            continue;
        };
        let Ok(keys) = parent.keys() else {
            continue;
        };

        for subkey_name in keys {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let Ok(subkey) = parent.open(&subkey_name) else {
                continue;
            };
            let Ok(display_name) = subkey.get_string("DisplayName") else {
                continue;
            };
            if display_name.trim().is_empty() {
                continue;
            }
            if subkey.get_u32("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            if subkey.get_string("ParentKeyName").is_ok() {
                continue;
            }
            if let Ok(release_type) = subkey.get_string("ReleaseType") {
                let release_type = release_type.to_ascii_lowercase();
                if release_type.contains("update")
                    || release_type.contains("hotfix")
                    || release_type.contains("security")
                {
                    continue;
                }
            }

            let command = subkey
                .get_string("UninstallString")
                .ok()
                .or_else(|| subkey.get_string("QuietUninstallString").ok());
            let Some(command) = command else {
                continue;
            };
            if command.to_ascii_lowercase().contains("windowsapps") {
                continue;
            }
            let Some(executable) = executable_path(&command) else {
                continue;
            };
            if !file_is_missing(&executable) {
                continue;
            }

            findings.push(RegistryFinding {
                category: RegistryCategoryId::Uninstall,
                hive: *hive,
                title: display_name,
                detail: format!("Не найден деинсталлятор: {}", executable.display()),
                key_display: format!("{}\\{}\\{}", hive.short(), parent_path, subkey_name),
                action: RegistryAction::DeleteKey {
                    parent_path: (*parent_path).to_owned(),
                    subkey_name,
                },
            });
        }
    }

    findings
}

fn scan_app_paths(cancel: &AtomicBool) -> Vec<RegistryFinding> {
    let mut findings = Vec::new();

    for (hive, parent_path) in APP_PATH_KEYS {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let Ok(parent) = hive.root().open(*parent_path) else {
            continue;
        };
        let Ok(keys) = parent.keys() else {
            continue;
        };

        for subkey_name in keys {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let Ok(subkey) = parent.open(&subkey_name) else {
                continue;
            };
            let Ok(default_value) = subkey.get_string("") else {
                continue;
            };
            let Some(executable) = executable_path(&default_value) else {
                continue;
            };
            if !file_is_missing(&executable) {
                continue;
            }

            findings.push(RegistryFinding {
                category: RegistryCategoryId::AppPaths,
                hive: *hive,
                title: subkey_name.clone(),
                detail: format!("Не найден файл: {}", executable.display()),
                key_display: format!("{}\\{}\\{}", hive.short(), parent_path, subkey_name),
                action: RegistryAction::DeleteKey {
                    parent_path: (*parent_path).to_owned(),
                    subkey_name,
                },
            });
        }
    }

    findings
}

fn executable_path(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    let raw = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next().unwrap_or_default()
    } else {
        command.split_whitespace().next().unwrap_or_default()
    };
    if raw.is_empty() {
        return None;
    }

    let expanded = expand_env(raw);
    let expanded = expanded.strip_prefix(r"\\?\").unwrap_or(&expanded);
    let path = PathBuf::from(expanded);

    if !path.is_absolute() || path.extension().is_none() {
        return None;
    }
    if path.to_string_lossy().starts_with(r"\\") {
        return None;
    }
    if let Some(root) = path.ancestors().last()
        && !root.exists()
    {
        return None;
    }

    Some(path)
}

fn file_is_missing(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) => !metadata.is_file(),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => false,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

fn expand_env(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];

        if let Some(end) = after.find('%') {
            let name = &after[..end];
            if name.is_empty() {
                output.push_str("%%");
            } else if let Some(value) = std::env::var_os(name) {
                output.push_str(&value.to_string_lossy());
            } else {
                output.push('%');
                output.push_str(name);
                output.push('%');
            }
            rest = &after[end + 1..];
        } else {
            output.push('%');
            output.push_str(after);
            rest = "";
        }
    }

    output.push_str(rest);
    output
}

#[derive(Debug, Clone)]
pub struct RegistryBackup {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum RegistryCleanEvent {
    Started {
        total: usize,
    },
    BackupCreated {
        dir: PathBuf,
    },
    Progress {
        done: usize,
        total: usize,
        deleted: usize,
        errors: usize,
        current: String,
        backup_dir: PathBuf,
    },
    Finished {
        deleted: usize,
        errors: usize,
        elapsed: Duration,
        cancelled: bool,
        backup_dir: PathBuf,
    },
    Failed(String),
}

pub fn spawn_clean(
    findings: Vec<RegistryFinding>,
    cancel: Arc<AtomicBool>,
    tx: Sender<RegistryCleanEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("chipclean-registry-clean".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let total = findings.len();
            let _ = tx.send(RegistryCleanEvent::Started { total });

            let mut journal = match Journal::open() {
                Ok(journal) => journal,
                Err(err) => {
                    let _ = tx.send(RegistryCleanEvent::Failed(format!(
                        "Не удалось открыть журнал операций: {err}"
                    )));
                    return;
                }
            };

            if let Err(err) = journal.record(json!({
                "ts": timestamp(),
                "event": "registry_cleanup_started",
                "findings": total,
            })) {
                let _ = tx.send(RegistryCleanEvent::Failed(format!(
                    "Не удалось записать начало очистки: {err}"
                )));
                return;
            }

            let backup = match create_backup(&findings, &config::backups_dir()) {
                Ok(backup) => backup,
                Err(err) => {
                    let _ = journal.record(json!({
                        "ts": timestamp(),
                        "event": "registry_backup_failed",
                        "error": err.to_string(),
                    }));
                    let _ = tx.send(RegistryCleanEvent::Failed(format!(
                        "Не удалось создать .reg-бэкап; очистка отменена: {err}"
                    )));
                    return;
                }
            };

            if let Err(err) = journal.record(json!({
                "ts": timestamp(),
                "event": "registry_backup",
                "dir": backup.dir.to_string_lossy(),
                "files": backup.files.len(),
            })) {
                let _ = tx.send(RegistryCleanEvent::Failed(format!(
                    "Не удалось записать журнал бэкапа: {err}"
                )));
                return;
            }

            let _ = tx.send(RegistryCleanEvent::BackupCreated {
                dir: backup.dir.clone(),
            });

            let mut deleted = 0_usize;
            let mut errors = 0_usize;
            let mut cancelled = false;

            for (index, finding) in findings.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }

                let result = delete_finding(finding);
                match &result {
                    Ok(()) => {
                        deleted += 1;
                        if let Err(err) = journal.record(json!({
                            "ts": timestamp(),
                            "event": "registry_delete",
                            "category": finding.category.key(),
                            "hive": finding.hive.short(),
                            "key": &finding.key_display,
                            "value": finding.value_name(),
                            "result": "ok",
                        })) {
                            let _ = tx.send(RegistryCleanEvent::Failed(format!(
                                "Журнал операций недоступен: {err}"
                            )));
                            return;
                        }
                    }
                    Err(err) => {
                        errors += 1;
                        if let Err(journal_err) = journal.record(json!({
                            "ts": timestamp(),
                            "event": "registry_delete",
                            "category": finding.category.key(),
                            "hive": finding.hive.short(),
                            "key": &finding.key_display,
                            "value": finding.value_name(),
                            "result": "error",
                            "error": err.to_string(),
                        })) {
                            let _ = tx.send(RegistryCleanEvent::Failed(format!(
                                "Журнал операций недоступен: {journal_err}"
                            )));
                            return;
                        }
                    }
                }

                let _ = tx.send(RegistryCleanEvent::Progress {
                    done: index + 1,
                    total,
                    deleted,
                    errors,
                    current: finding.key_display.clone(),
                    backup_dir: backup.dir.clone(),
                });
            }

            let _ = journal.record(json!({
                "ts": timestamp(),
                "event": "registry_cleanup_finished",
                "deleted": deleted,
                "errors": errors,
                "cancelled": cancelled,
                "elapsed_ms": started.elapsed().as_millis(),
            }));
            let _ = journal.flush();

            let _ = tx.send(RegistryCleanEvent::Finished {
                deleted,
                errors,
                elapsed: started.elapsed(),
                cancelled,
                backup_dir: backup.dir,
            });
        })
        .expect("не удалось создать поток очистки реестра")
}

fn create_backup(findings: &[RegistryFinding], base: &Path) -> Result<RegistryBackup> {
    let dir = base.join(format!(
        "registry-{}",
        Utc::now().format("%Y%m%d-%H%M%S-%3f")
    ));
    fs::create_dir_all(&dir).with_context(|| format!("не удалось создать {}", dir.display()))?;

    let mut exported = std::collections::BTreeMap::<String, PathBuf>::new();

    for finding in findings {
        let (hive, key_path) = finding.backup_target();
        let full = format!("{}\\{}", hive.reg_name(), key_path);
        if exported.contains_key(&full) {
            continue;
        }

        let filename = format!(
            "{:03}-{}.reg",
            exported.len() + 1,
            sanitize_filename(&key_path)
        );
        let path = export_key_to(hive, &key_path, &dir, filename)?;
        exported.insert(full, path);
    }

    Ok(RegistryBackup {
        dir,
        files: exported.into_values().collect(),
    })
}

pub(crate) fn export_registry_key(
    hive: RegistryHive,
    key_path: &str,
    base_dir: &Path,
    tag: &str,
) -> Result<PathBuf> {
    let dir = base_dir.join(format!("{tag}-{}", Utc::now().format("%Y%m%d-%H%M%S-%3f")));
    fs::create_dir_all(&dir).with_context(|| format!("не удалось создать {}", dir.display()))?;

    let filename = format!(
        "{}-{}.reg",
        sanitize_filename(tag),
        sanitize_filename(key_path)
    );
    export_key_to(hive, key_path, &dir, filename)
}

fn export_key_to(
    hive: RegistryHive,
    key_path: &str,
    dir: &Path,
    filename: String,
) -> Result<PathBuf> {
    let full = format!("{}\\{}", hive.reg_name(), key_path);
    let path = dir.join(filename);
    let text = crate::core::registry_export::export_key_text(hive.root(), hive.reg_name(), key_path)
        .with_context(|| format!("не удалось сформировать экспорт для {full}"))?;
    fs::write(&path, text)
        .with_context(|| format!("не удалось записать экспорт в {}", path.display()))?;
    Ok(path)
}

fn delete_finding(finding: &RegistryFinding) -> Result<()> {
    match &finding.action {
        RegistryAction::DeleteValue {
            key_path,
            value_name,
        } => {
            let key = finding
                .hive
                .root()
                .options()
                .read()
                .write()
                .open(key_path)
                .map_err(|err| anyhow!("не удалось открыть {}: {err}", finding.key_display))?;
            key.remove_value(value_name)
                .map_err(|err| anyhow!("не удалось удалить значение: {err}"))?;
        }
        RegistryAction::DeleteKey {
            parent_path,
            subkey_name,
        } => {
            let parent = finding
                .hive
                .root()
                .options()
                .read()
                .write()
                .open(parent_path)
                .map_err(|err| anyhow!("не удалось открыть {}: {err}", finding.key_display))?;
            parent
                .remove_tree(subkey_name)
                .map_err(|err| anyhow!("не удалось удалить ключ: {err}"))?;
        }
    }

    Ok(())
}

fn sanitize_filename(path: &str) -> String {
    let mut output: String = path
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .take(100)
        .collect();

    if output.is_empty() {
        output.push_str("key");
    }

    output
}

fn timestamp() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_registry::CURRENT_USER;

    #[test]
    fn extracts_executable_from_startup_command() {
        let parsed = executable_path(r#""C:\Program Files\App\app.exe" --min"#);
        assert_eq!(parsed, Some(PathBuf::from(r"C:\Program Files\App\app.exe")));

        assert_eq!(executable_path("MsiExec.exe /X{123}"), None);
        assert_eq!(
            executable_path("rundll32.exe shell32.dll,Control_RunDLL"),
            None
        );
    }

    #[test]
    fn expands_environment_variables() {
        let expanded = expand_env(r"%WINDIR%\System32\notepad.exe");
        assert!(!expanded.contains('%'));
        assert!(expanded.to_lowercase().ends_with(r"\system32\notepad.exe"));
    }

    #[test]
    fn backs_up_and_deletes_a_test_value() {
        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanTest-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let test_path = format!(r"Software\ChipCleanTests\{unique}");

        let key = CURRENT_USER.create(&test_path).unwrap();
        key.set_string("RemoveMe", "bye").unwrap();
        key.set_string("KeepMe", "stay").unwrap();

        let finding = RegistryFinding {
            category: RegistryCategoryId::Startup,
            hive: RegistryHive::CurrentUser,
            title: "RemoveMe".to_owned(),
            detail: "test".to_owned(),
            key_display: format!("HKCU\\{test_path}"),
            action: RegistryAction::DeleteValue {
                key_path: test_path.clone(),
                value_name: "RemoveMe".to_owned(),
            },
        };

        let backup_dir = tempfile::tempdir().unwrap();
        let backup = create_backup(std::slice::from_ref(&finding), backup_dir.path()).unwrap();
        assert!(backup.dir.is_dir());
        assert_eq!(backup.files.len(), 1);
        assert!(backup.files[0].metadata().unwrap().len() > 0);

        delete_finding(&finding).unwrap();

        let reopened = CURRENT_USER.open(&test_path).unwrap();
        assert!(reopened.get_string("RemoveMe").is_err());
        assert_eq!(reopened.get_string("KeepMe").unwrap(), "stay");

        drop(reopened);
        drop(key);
        let _ = CURRENT_USER.remove_tree(&test_path);
    }

    #[test]
    fn detects_missing_files_conservatively() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present.exe");
        std::fs::write(&present, b"exe").unwrap();

        assert!(!file_is_missing(&present));
        assert!(file_is_missing(&dir.path().join("missing.exe")));
        assert!(file_is_missing(dir.path()));
    }

    #[test]
    fn reads_default_registry_value() {
        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanDefault-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let test_path = format!(r"Software\ChipCleanTests\{unique}");
        let key = CURRENT_USER.create(&test_path).unwrap();

        key.set_string("", r"C:\Windows\System32\notepad.exe")
            .unwrap();
        assert_eq!(
            key.get_string("").unwrap(),
            r"C:\Windows\System32\notepad.exe"
        );

        drop(key);
        let _ = CURRENT_USER.remove_tree(&test_path);
    }
}
