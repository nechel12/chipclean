use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::{
    config,
    core::{
        journal::Journal,
        registry_editor::{self, EditorHive},
        startup::{self, StartupEntry},
        traces::{self, QuarantinedTrace},
    },
};

const BACKUP_LIMIT: usize = 500;
const BACKUP_PARSE_BYTES: u64 = 4 * 1024 * 1024;
const TARGET_LIMIT: usize = 48;
const JOURNAL_LIMIT: usize = 60;
const WALK_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupTarget {
    pub hive: EditorHive,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RegistryBackup {
    pub path: PathBuf,
    pub label: String,
    pub group: String,
    pub created_at: Option<DateTime<Utc>>,
    pub size: u64,
    pub targets: Vec<BackupTarget>,
    pub truncated: bool,
}

impl RegistryBackup {
    pub fn target_summary(&self) -> String {
        match self.targets.first() {
            Some(target) if self.targets.len() == 1 => {
                format!("{}\\{}", target.hive.short(), target.path)
            }
            Some(target) => format!(
                "{}\\{} · ещё {}",
                target.hive.short(),
                target.path,
                self.targets.len() - 1
            ),
            None => "Разделы не обнаружены".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct JournalEvent {
    pub timestamp: Option<DateTime<Utc>>,
    pub summary: String,
    pub detail: Option<String>,
    pub backup: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct Inventory {
    pub registry: Vec<RegistryBackup>,
    pub traces: Vec<QuarantinedTrace>,
    pub startup: Vec<StartupEntry>,
    pub journal: Vec<JournalEvent>,
    pub errors: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct RestoreReport {
    pub safety_backups: Vec<PathBuf>,
}

pub fn scan_inventory() -> Inventory {
    let mut inventory = Inventory::default();

    match scan_registry_backups() {
        Ok((backups, truncated)) => {
            inventory.registry = backups;
            inventory.truncated = truncated;
        }
        Err(err) => inventory
            .errors
            .push(format!("Не удалось прочитать .reg-бэкапы: {err}")),
    }

    match traces::load_quarantine() {
        Ok(store) => inventory.traces = store.entries,
        Err(err) => inventory
            .errors
            .push(format!("Не удалось прочитать карантин следов: {err}")),
    }

    match startup::scan_entries() {
        Ok(entries) => {
            inventory.startup = entries.into_iter().filter(|entry| !entry.enabled).collect()
        }
        Err(err) => inventory.errors.push(format!(
            "Не удалось прочитать отключённую автозагрузку: {err}"
        )),
    }

    match recent_journal(JOURNAL_LIMIT) {
        Ok(events) => inventory.journal = events,
        Err(err) => inventory
            .errors
            .push(format!("Не удалось прочитать журнал операций: {err}")),
    }

    inventory
}

fn scan_registry_backups() -> Result<(Vec<RegistryBackup>, bool)> {
    let root = config::backups_dir();
    if !root.is_dir() {
        return Ok((Vec::new(), false));
    }

    let mut backups = Vec::new();
    let mut truncated = false;

    for entry in WalkDir::new(&root)
        .max_depth(WALK_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if backups.len() >= BACKUP_LIMIT {
            truncated = true;
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if !entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("reg"))
        {
            continue;
        }
        match read_backup(&root, entry.path()) {
            Ok(backup) => backups.push(backup),
            Err(err) => {
                tracing::warn!(%err, path = %entry.path().display(), "failed to read registry backup");
            }
        }
    }

    backups.sort_by_key(|backup| std::cmp::Reverse(backup.created_at));
    Ok((backups, truncated))
}

fn read_backup(root: &Path, path: &Path) -> Result<RegistryBackup> {
    let metadata =
        fs::metadata(path).with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let relative = path.strip_prefix(root).unwrap_or(path);
    let group = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_else(|| "backups".to_owned());

    let (targets, truncated) = parse_targets(path)?;

    Ok(RegistryBackup {
        path: path.to_path_buf(),
        label: relative.to_string_lossy().to_string(),
        group,
        created_at: metadata.modified().ok().map(DateTime::<Utc>::from),
        size: metadata.len(),
        targets,
        truncated,
    })
}

fn parse_targets(path: &Path) -> Result<(Vec<BackupTarget>, bool)> {
    let mut file =
        File::open(path).with_context(|| format!("не удалось открыть {}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(BACKUP_PARSE_BYTES)
        .read_to_end(&mut bytes)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;

    let text = decode_reg_text(&bytes);
    Ok(parse_targets_from_text(&text))
}

fn decode_reg_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let mut body = &bytes[2..];
        body = &body[..body.len() - body.len() % 2];
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        let mut body = &bytes[2..];
        body = &body[..body.len() - body.len() % 2];
        let units: Vec<u16> = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn parse_targets_from_text(text: &str) -> (Vec<BackupTarget>, bool) {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    let mut truncated = false;

    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(']') else {
            continue;
        };
        let key = rest.trim_start_matches('-').trim_matches('\\');
        let Some(target) = parse_reg_target(key) else {
            continue;
        };
        let identity = format!(
            "{}\\{}",
            target.hive.short(),
            target.path.to_ascii_lowercase()
        );
        if !seen.insert(identity) {
            continue;
        }
        if targets.len() >= TARGET_LIMIT {
            truncated = true;
            break;
        }
        targets.push(target);
    }

    (targets, truncated)
}

fn parse_reg_target(key: &str) -> Option<BackupTarget> {
    let (hive, path) = key.split_once('\\').unwrap_or((key, ""));
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKEY_CURRENT_USER" | "HKCU" => EditorHive::Hkcu,
        "HKEY_LOCAL_MACHINE" | "HKLM" => EditorHive::Hklm,
        "HKEY_CLASSES_ROOT" | "HKCR" => EditorHive::Hkcr,
        "HKEY_USERS" | "HKU" => EditorHive::Hku,
        "HKEY_CURRENT_CONFIG" | "HKCC" => EditorHive::Hkcc,
        _ => return None,
    };
    Some(BackupTarget {
        hive,
        path: path.to_owned(),
    })
}

pub fn restore_registry(backup: &RegistryBackup) -> Result<RestoreReport> {
    restore_registry_at(backup, &config::backups_dir())
}

fn restore_registry_at(backup: &RegistryBackup, backups_root: &Path) -> Result<RestoreReport> {
    if backup.targets.is_empty() {
        bail!("В файле .reg не удалось определить разделы; восстановление отменено");
    }
    ensure_inside(backup.path.as_path(), backups_root)?;

    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let safety_dir = backups_root.join(format!("pre-restore-{stamp}"));
    let mut report = RestoreReport::default();
    let mut exported = BTreeSet::new();

    for target in &backup.targets {
        if registry_editor::open_key(target.hive, &target.path).is_err() {
            continue;
        }
        let identity = format!(
            "{}\\{}",
            target.hive.short(),
            target.path.to_ascii_lowercase()
        );
        if !exported.insert(identity) {
            continue;
        }
        let path = registry_editor::backup_key_to(target.hive, &target.path, &safety_dir)?;
        report.safety_backups.push(path);
    }

    registry_editor::import_reg_file(&backup.path)?;

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "backup_restored",
        "backup": backup.path.display().to_string(),
        "keys": backup.targets.iter().map(|target| format!("{}\\{}", target.hive.short(), target.path)).collect::<Vec<_>>(),
        "safety_backups": report.safety_backups.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
    }));

    Ok(report)
}

pub fn restore_trace(entry: &QuarantinedTrace) -> Result<String> {
    traces::restore_quarantined(entry)?;
    Ok(format!(
        "Возвращено на место: {}",
        entry.original_path.display()
    ))
}

pub fn restore_startup(entry: &StartupEntry) -> Result<String> {
    if entry.enabled {
        bail!("Запись автозагрузки уже включена");
    }
    startup::toggle_entry(entry)
}

fn ensure_inside(path: &Path, root: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("Файл бэкапа не найден: {}", path.display());
    }
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("не удалось проверить {}", root.display()))?;
    let canonical_path = fs::canonicalize(path)
        .with_context(|| format!("не удалось проверить {}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!("Файл бэкапа находится вне каталога бэкапов");
    }
    Ok(())
}

pub fn recent_journal(limit: usize) -> Result<Vec<JournalEvent>> {
    let path = config::journal_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось прочитать {}", path.display()));
        }
    };
    Ok(parse_journal(&raw, limit))
}

fn parse_journal(raw: &str, limit: usize) -> Vec<JournalEvent> {
    raw.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| journal_event(&value))
        .take(limit)
        .collect()
}

fn journal_event(value: &Value) -> Option<JournalEvent> {
    let event = value.get("event")?.as_str()?;
    let timestamp = value
        .get("ts")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|parsed| parsed.with_timezone(&Utc));

    let (summary, detail) = summary_for(event, value);
    Some(JournalEvent {
        timestamp,
        summary,
        detail,
        backup: value
            .get("backup")
            .and_then(Value::as_str)
            .map(PathBuf::from),
    })
}

fn summary_for(event: &str, value: &Value) -> (String, Option<String>) {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let first_detail = || {
        ["path", "original", "key", "title", "program", "quarantine"]
            .iter()
            .find_map(|key| text(key))
    };

    let summary = match event {
        "cleanup_started" => "Запущена очистка файлов".to_owned(),
        "cleanup_finished" => {
            let files = value.get("files").and_then(Value::as_u64).unwrap_or(0);
            let bytes = value.get("bytes").and_then(Value::as_u64).unwrap_or(0);
            format!("Очистка завершена: {files} файлов · {}", human_bytes(bytes))
        }
        "delete" if value.get("result").and_then(Value::as_str) == Some("error") => {
            "Не удалось удалить файл".to_owned()
        }
        "delete" => "Удалён файл".to_owned(),
        "empty_recycle_bin" => "Очищена корзина".to_owned(),
        "privacy_clean" => "Очищены личные данные".to_owned(),
        "chipclean_quarantine_purged" => "Очищен карантин".to_owned(),
        "registry_cleanup_started" => "Запущена чистка реестра".to_owned(),
        "registry_backup" => "Создан .reg-бэкап реестра".to_owned(),
        "registry_backup_failed" => "Не удалось создать .reg-бэкап".to_owned(),
        "registry_delete" => "Удалён ключ или значение реестра".to_owned(),
        "registry_cleanup_finished" => "Чистка реестра завершена".to_owned(),
        "registry_value_set" => "Изменено значение реестра".to_owned(),
        "registry_value_deleted" => "Удалено значение реестра".to_owned(),
        "registry_key_created" => "Создан раздел реестра".to_owned(),
        "registry_key_deleted" => "Удалён раздел реестра".to_owned(),
        "backup_restored" => "Восстановление из .reg-бэкапа".to_owned(),
        "service_startup_disabled" => "Служба переведена в ручной запуск".to_owned(),
        "service_startup_enabled" => "Служба переведена в автоматический запуск".to_owned(),
        "service_restored_manual" => "Служба возвращена в ручной режим".to_owned(),
        "startup_disabled" => "Отключена запись автозагрузки".to_owned(),
        "startup_enabled" => "Включена запись автозагрузки".to_owned(),
        "startup_folder_disabled" => "Элемент автозагрузки перемещён в карантин".to_owned(),
        "startup_folder_enabled" => "Элемент автозагрузки возвращён".to_owned(),
        "trace_quarantined" => "След программы перемещён в карантин".to_owned(),
        "trace_restored" => "След программы возвращён".to_owned(),
        "trace_deleted_permanently" => "След программы удалён безвозвратно".to_owned(),
        "trace_delete_failed" => "Не удалось удалить след программы".to_owned(),
        "program_uninstaller_started" => "Запущен штатный деинсталлятор".to_owned(),
        other => other.to_owned(),
    };

    (summary, first_detail())
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["Б", "КБ", "МБ", "ГБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn journal(event: Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write rollback journal event");
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

    #[test]
    fn parses_utf16_targets() {
        let text = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\ChipClean\\Test]\r\n\"a\"=\"b\"\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let decoded = decode_reg_text(&bytes);
        let (targets, truncated) = parse_targets_from_text(&decoded);

        assert!(!truncated);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].hive, EditorHive::Hkcu);
        assert_eq!(targets[0].path, r"Software\ChipClean\Test");
    }

    #[test]
    fn deduplicates_and_skips_unknown_hives() {
        let text = "[HKEY_CURRENT_USER\\Software\\A]\n[HKCU\\Software\\A]\n[HKEY_FAKE\\X]\n[-HKEY_LOCAL_MACHINE\\SOFTWARE\\B]\n";
        let (targets, truncated) = parse_targets_from_text(text);

        assert!(!truncated);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[1].hive, EditorHive::Hklm);
        assert_eq!(targets[1].path, r"SOFTWARE\B");
    }

    #[test]
    fn parses_journal_lines_newest_first() {
        let raw = concat!(
            "{\"ts\":\"2026-01-01T10:00:00Z\",\"event\":\"cleanup_started\"}\n",
            "{\"ts\":\"2026-01-01T10:01:00Z\",\"event\":\"delete\",\"path\":\"C:\\\\Temp\\\\a.tmp\",\"result\":\"ok\"}\n",
            "not json\n"
        );
        let events = parse_journal(raw, 10);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].summary, "Удалён файл");
        assert_eq!(events[0].detail.as_deref(), Some(r"C:\Temp\a.tmp"));
        assert_eq!(events[1].summary, "Запущена очистка файлов");
    }

    #[test]
    fn restores_a_reg_export() {
        let _guard = crate::core::registry_test_guard();
        use windows_registry::CURRENT_USER;

        let unique = format!(
            "ChipCleanRollback-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&path).unwrap();
        key.set_string("Value", "before").unwrap();
        drop(key);

        let dir = tempfile::tempdir().unwrap();
        let backup_path =
            registry_editor::backup_key_to(EditorHive::Hkcu, &path, dir.path()).unwrap();
        let backup = RegistryBackup {
            path: backup_path,
            label: "test".to_owned(),
            group: "test".to_owned(),
            created_at: None,
            size: 0,
            targets: vec![BackupTarget {
                hive: EditorHive::Hkcu,
                path: path.clone(),
            }],
            truncated: false,
        };

        let key = CURRENT_USER.options().read().write().open(&path).unwrap();
        key.set_string("Value", "after").unwrap();
        drop(key);

        let report = restore_registry_at(&backup, dir.path()).unwrap();
        assert!(!report.safety_backups.is_empty());

        let key = CURRENT_USER.open(&path).unwrap();
        assert_eq!(key.get_string("Value").unwrap(), "before");
        drop(key);
        let _ = CURRENT_USER.remove_tree(&path);
    }
}
