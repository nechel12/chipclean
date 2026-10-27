use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use windows_registry::Type;

use crate::{
    config,
    core::{
        journal::Journal,
        path_guard,
        registry::{self, RegistryHive, STARTUP_KEYS},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupSource {
    HkcuRun,
    HklmRun,
    UserFolder,
    CommonFolder,
}

impl StartupSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::HkcuRun => "HKCU Run",
            Self::HklmRun => "HKLM Run",
            Self::UserFolder => "Папка пользователя",
            Self::CommonFolder => "Общая папка",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::HkcuRun => "HKCU",
            Self::HklmRun => "HKLM",
            Self::UserFolder => "Пользователь",
            Self::CommonFolder => "Общая",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DisabledKind {
    Registry {
        hive: String,
        key_path: String,
        value_name: String,
        value_type: u32,
        value_bytes: Vec<u8>,
    },
    Folder {
        original_path: PathBuf,
        stored_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisabledEntry {
    pub source: StartupSource,
    pub title: String,
    pub command: String,
    pub disabled_at: String,
    pub kind: DisabledKind,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DisabledStore {
    pub entries: Vec<DisabledEntry>,
}

#[derive(Debug, Clone)]
pub enum StartupEntryKind {
    Registry {
        hive: RegistryHive,
        key_path: String,
        value_name: String,
    },
    Folder {
        path: PathBuf,
    },
    Disabled(DisabledEntry),
}

#[derive(Debug, Clone)]
pub struct StartupEntry {
    pub title: String,
    pub command: String,
    pub source: StartupSource,
    pub enabled: bool,
    pub kind: StartupEntryKind,
}

pub fn scan_entries() -> Result<Vec<StartupEntry>> {
    scan_entries_with_store(&config::startup_store_path())
}

fn scan_entries_with_store(store_path: &Path) -> Result<Vec<StartupEntry>> {
    let store = load_store(store_path)?;
    let mut entries = Vec::new();
    let mut disabled_registry = HashSet::new();
    let mut disabled_folders = HashSet::new();

    for disabled in &store.entries {
        match &disabled.kind {
            DisabledKind::Registry {
                hive,
                key_path,
                value_name,
                ..
            } => {
                disabled_registry.insert((hive.clone(), key_path.clone(), value_name.clone()));
            }
            DisabledKind::Folder { original_path, .. } => {
                disabled_folders.insert(original_path.clone());
            }
        }
    }

    for (hive, key_path) in STARTUP_KEYS {
        let Ok(key) = hive.root().open(*key_path) else {
            continue;
        };
        let Ok(values) = key.values() else {
            continue;
        };

        for (name, _) in values {
            let id = (
                hive.short().to_owned(),
                (*key_path).to_owned(),
                name.clone(),
            );
            if disabled_registry.contains(&id) {
                continue;
            }

            let command = key.get_string(&name).unwrap_or_default();
            entries.push(StartupEntry {
                title: name.clone(),
                command,
                source: if *hive == RegistryHive::CurrentUser {
                    StartupSource::HkcuRun
                } else {
                    StartupSource::HklmRun
                },
                enabled: true,
                kind: StartupEntryKind::Registry {
                    hive: *hive,
                    key_path: (*key_path).to_owned(),
                    value_name: name,
                },
            });
        }
    }

    for (source, folder) in startup_folders() {
        let Ok(read_dir) = fs::read_dir(&folder) else {
            continue;
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            if disabled_folders.contains(&path) {
                continue;
            }

            entries.push(StartupEntry {
                title: file_name,
                command: path.display().to_string(),
                source,
                enabled: true,
                kind: StartupEntryKind::Folder { path },
            });
        }
    }

    for disabled in store.entries {
        entries.push(StartupEntry {
            title: disabled.title.clone(),
            command: disabled.command.clone(),
            source: disabled.source,
            enabled: false,
            kind: StartupEntryKind::Disabled(disabled),
        });
    }

    entries.sort_by(|left, right| {
        right
            .enabled
            .cmp(&left.enabled)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });

    Ok(entries)
}

pub fn toggle_entry(entry: &StartupEntry) -> Result<String> {
    let startup_roots: Vec<PathBuf> = startup_folders()
        .into_iter()
        .map(|(_, folder)| folder)
        .collect();
    toggle_entry_with_paths(
        entry,
        &config::startup_store_path(),
        &config::backups_dir(),
        &config::startup_quarantine_dir(),
        &startup_roots,
    )
}

fn toggle_entry_with_paths(
    entry: &StartupEntry,
    store_path: &Path,
    backup_dir: &Path,
    quarantine_dir: &Path,
    startup_roots: &[PathBuf],
) -> Result<String> {
    match (&entry.kind, entry.enabled) {
        (
            StartupEntryKind::Registry {
                hive,
                key_path,
                value_name,
            },
            true,
        ) => disable_registry(*hive, key_path, value_name, store_path, backup_dir),
        (StartupEntryKind::Folder { path }, true) => {
            disable_folder(path, store_path, quarantine_dir, startup_roots)
        }
        (StartupEntryKind::Disabled(disabled), false) => {
            enable_disabled(disabled, store_path, quarantine_dir, startup_roots)
        }
        _ => bail!("Состояние записи изменилось; обновите список."),
    }
}

fn disable_registry(
    hive: RegistryHive,
    key_path: &str,
    value_name: &str,
    store_path: &Path,
    backup_dir: &Path,
) -> Result<String> {
    let key = hive
        .root()
        .options()
        .read()
        .write()
        .open(key_path)
        .map_err(|err| anyhow!("не удалось открыть {}: {err}", hive.short()))?;

    let value = key
        .get_value(value_name)
        .map_err(|err| anyhow!("не удалось прочитать значение: {err}"))?;
    let value_type = value.ty();
    let value_bytes = value.as_ref().to_vec();
    let command = key.get_string(value_name).unwrap_or_default();

    let backup = registry::export_registry_key(hive, key_path, backup_dir, "startup")?;

    key.remove_value(value_name)
        .map_err(|err| anyhow!("не удалось удалить значение автозагрузки: {err}"))?;

    let source = if hive == RegistryHive::CurrentUser {
        StartupSource::HkcuRun
    } else {
        StartupSource::HklmRun
    };
    let mut store = load_store(store_path)?;
    store.entries.push(DisabledEntry {
        source,
        title: value_name.to_owned(),
        command,
        disabled_at: now(),
        kind: DisabledKind::Registry {
            hive: hive.short().to_owned(),
            key_path: key_path.to_owned(),
            value_name: value_name.to_owned(),
            value_type: u32::from(value_type),
            value_bytes: value_bytes.clone(),
        },
    });

    if let Err(err) = save_store(store_path, &store) {
        let restore = key.set_bytes(value_name, value_type, &value_bytes);
        return match restore {
            Ok(()) => Err(err.context("не удалось сохранить состояние; значение восстановлено")),
            Err(restore_err) => Err(anyhow!(
                "не удалось сохранить состояние: {err}; восстановление не удалось: {restore_err}; бэкап: {}",
                backup.display()
            )),
        };
    }

    journal(json!({
        "ts": now(),
        "event": "startup_disabled",
        "source": source.label(),
        "title": value_name,
        "key": format!("{}\\{}", hive.short(), key_path),
        "backup": backup.display().to_string(),
    }));

    Ok(format!("Отключено. Бэкап: {}", backup.display()))
}

fn disable_folder(
    path: &Path,
    store_path: &Path,
    quarantine_dir: &Path,
    startup_roots: &[PathBuf],
) -> Result<String> {
    if !path.is_file() {
        bail!("Элемент автозагрузки не является файлом");
    }
    path_guard::ensure_not_reparse(path)?;
    path_guard::ensure_direct_child(path, startup_roots)?;

    fs::create_dir_all(quarantine_dir)
        .with_context(|| format!("не удалось создать {}", quarantine_dir.display()))?;

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "startup-item".to_owned());
    let stored_path = unique_quarantine_path(quarantine_dir, &file_name);

    fs::rename(path, &stored_path)
        .with_context(|| format!("не удалось переместить {}", path.display()))?;

    let source = source_for_folder(path);
    let mut store = load_store(store_path)?;
    store.entries.push(DisabledEntry {
        source,
        title: file_name,
        command: path.display().to_string(),
        disabled_at: now(),
        kind: DisabledKind::Folder {
            original_path: path.to_path_buf(),
            stored_path: stored_path.clone(),
        },
    });

    if let Err(err) = save_store(store_path, &store) {
        let restore = fs::rename(&stored_path, path);
        return match restore {
            Ok(()) => Err(err.context("не удалось сохранить состояние; файл возвращён")),
            Err(restore_err) => Err(anyhow!(
                "не удалось сохранить состояние: {err}; возврат файла не удался: {restore_err}; файл: {}",
                stored_path.display()
            )),
        };
    }

    journal(json!({
        "ts": now(),
        "event": "startup_folder_disabled",
        "source": source.label(),
        "title": path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default(),
        "original": path.display().to_string(),
        "quarantine": stored_path.display().to_string(),
    }));

    Ok(format!("Перемещено в карантин: {}", stored_path.display()))
}

fn enable_disabled(
    disabled: &DisabledEntry,
    store_path: &Path,
    quarantine_dir: &Path,
    startup_roots: &[PathBuf],
) -> Result<String> {
    let mut store = load_store(store_path)?;
    let position = store
        .entries
        .iter()
        .position(|entry| entry == disabled)
        .ok_or_else(|| anyhow!("Запись не найдена в хранилище; обновите список."))?;

    match &disabled.kind {
        DisabledKind::Registry {
            hive,
            key_path,
            value_name,
            value_type,
            value_bytes,
        } => {
            let hive = parse_hive(hive)?;
            let key = hive
                .root()
                .create(key_path)
                .map_err(|err| anyhow!("не удалось открыть {}: {err}", hive.short()))?;

            if key.get_value(value_name).is_ok() {
                bail!("Значение уже существует в реестре; включение отменено.");
            }

            let value_type = Type::from(*value_type);
            key.set_bytes(value_name, value_type, value_bytes)
                .map_err(|err| anyhow!("не удалось восстановить значение: {err}"))?;

            store.entries.remove(position);
            if let Err(err) = save_store(store_path, &store) {
                let _ = key.remove_value(value_name);
                return Err(err.context("не удалось сохранить состояние; значение удалено"));
            }

            journal(json!({
                "ts": now(),
                "event": "startup_enabled",
                "source": disabled.source.label(),
                "title": disabled.title,
                "key": format!("{}\\{}", hive.short(), key_path),
            }));

            Ok("Включено".to_owned())
        }
        DisabledKind::Folder {
            original_path,
            stored_path,
        } => {
            if original_path.exists() {
                bail!("Файл уже существует: {}", original_path.display());
            }
            if !stored_path.is_file() {
                bail!("Файл карантина не найден: {}", stored_path.display());
            }
            path_guard::ensure_not_reparse(stored_path)?;
            let quarantine_roots = [quarantine_dir.to_path_buf()];
            path_guard::ensure_existing_within(stored_path, &quarantine_roots)?;

            let destination = path_guard::canonical_destination(original_path)?;
            path_guard::ensure_direct_child(&destination, startup_roots)?;

            fs::rename(stored_path, &destination).with_context(|| {
                format!(
                    "не удалось вернуть {} на место {}",
                    stored_path.display(),
                    destination.display()
                )
            })?;

            store.entries.remove(position);
            if let Err(err) = save_store(store_path, &store) {
                let _ = fs::rename(&destination, stored_path);
                return Err(
                    err.context("не удалось сохранить состояние; файл возвращён в карантин")
                );
            }

            journal(json!({
                "ts": now(),
                "event": "startup_folder_enabled",
                "source": disabled.source.label(),
                "title": disabled.title,
                "original": destination.display().to_string(),
            }));

            Ok("Включено".to_owned())
        }
    }
}

pub fn purge_folder_quarantine_metadata() -> Result<()> {
    let path = config::startup_store_path();
    let mut store = load_store(&path)?;
    let before = store.entries.len();
    store
        .entries
        .retain(|entry| matches!(entry.kind, DisabledKind::Registry { .. }));
    if store.entries.len() != before {
        save_store(&path, &store)?;
    }
    Ok(())
}
fn load_store(path: &Path) -> Result<DisabledStore> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(DisabledStore::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось прочитать {}", path.display()));
        }
    };

    serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("не удалось разобрать {}", path.display()))
}

fn save_store(path: &Path, store: &DisabledStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("не удалось создать {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(store)?;
    fs::write(path, raw).with_context(|| format!("не удалось записать {}", path.display()))
}

fn startup_folders() -> Vec<(StartupSource, PathBuf)> {
    let mut folders = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        folders.push((
            StartupSource::UserFolder,
            PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
        ));
    }
    if let Some(programdata) = std::env::var_os("ProgramData") {
        folders.push((
            StartupSource::CommonFolder,
            PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
        ));
    }

    folders
}

fn source_for_folder(path: &Path) -> StartupSource {
    let text = path.to_string_lossy().to_lowercase();
    let common = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .map(|path| path.join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
        .map(|path| path.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if !common.is_empty() && text.starts_with(&common) {
        StartupSource::CommonFolder
    } else {
        StartupSource::UserFolder
    }
}

fn unique_quarantine_path(dir: &Path, file_name: &str) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let mut candidate = dir.join(format!("{stamp}-{file_name}"));
    let mut counter = 1_u32;

    while candidate.exists() {
        candidate = dir.join(format!("{stamp}-{counter}-{file_name}"));
        counter += 1;
    }

    candidate
}

fn parse_hive(value: &str) -> Result<RegistryHive> {
    match value {
        "HKCU" => Ok(RegistryHive::CurrentUser),
        "HKLM" => Ok(RegistryHive::LocalMachine),
        other => bail!("неизвестная ветка реестра: {other}"),
    }
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write startup journal event");
            } else {
                let _ = journal.flush();
            }
        }
        Err(err) => tracing::warn!(%err, "failed to open operations journal"),
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_registry::CURRENT_USER;

    #[test]
    fn store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disabled-startup.json");
        let mut store = DisabledStore::default();
        store.entries.push(DisabledEntry {
            source: StartupSource::HkcuRun,
            title: "Test".to_owned(),
            command: "test.exe".to_owned(),
            disabled_at: now(),
            kind: DisabledKind::Registry {
                hive: "HKCU".to_owned(),
                key_path: r"Software\Test\Run".to_owned(),
                value_name: "Test".to_owned(),
                value_type: 1,
                value_bytes: vec![1, 2, 3],
            },
        });

        save_store(&path, &store).unwrap();
        let loaded = load_store(&path).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].title, "Test");
    }

    #[test]
    fn disables_and_enables_registry_value() {
        let unique = format!(
            "ChipCleanStartup-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let test_root = format!(r"Software\ChipCleanTests-Startup-{unique}");
        let key_path = format!(r"{test_root}\Run");
        let key = CURRENT_USER.create(&key_path).unwrap();
        key.set_string("TestApp", r"C:\Windows\System32\notepad.exe")
            .unwrap();

        let entry = StartupEntry {
            title: "TestApp".to_owned(),
            command: r"C:\Windows\System32\notepad.exe".to_owned(),
            source: StartupSource::HkcuRun,
            enabled: true,
            kind: StartupEntryKind::Registry {
                hive: RegistryHive::CurrentUser,
                key_path: key_path.clone(),
                value_name: "TestApp".to_owned(),
            },
        };

        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("store.json");
        let backup_dir = dir.path().join("backups");
        let quarantine_dir = dir.path().join("quarantine");

        let message =
            toggle_entry_with_paths(&entry, &store_path, &backup_dir, &quarantine_dir, &[])
                .unwrap();
        assert!(message.contains("Отключено"));
        assert!(
            CURRENT_USER
                .open(&key_path)
                .unwrap()
                .get_string("TestApp")
                .is_err()
        );

        let store = load_store(&store_path).unwrap();
        assert_eq!(store.entries.len(), 1);
        let backup_files: Vec<_> = fs::read_dir(&backup_dir).unwrap().flatten().collect();
        assert!(!backup_files.is_empty());

        let disabled = StartupEntry {
            title: store.entries[0].title.clone(),
            command: store.entries[0].command.clone(),
            source: store.entries[0].source,
            enabled: false,
            kind: StartupEntryKind::Disabled(store.entries[0].clone()),
        };

        let message =
            toggle_entry_with_paths(&disabled, &store_path, &backup_dir, &quarantine_dir, &[])
                .unwrap();
        assert_eq!(message, "Включено");
        assert_eq!(
            CURRENT_USER
                .open(&key_path)
                .unwrap()
                .get_string("TestApp")
                .unwrap(),
            r"C:\Windows\System32\notepad.exe"
        );
        assert!(load_store(&store_path).unwrap().entries.is_empty());

        drop(key);
        let _ = CURRENT_USER.remove_tree(&test_root);
    }

    #[test]
    fn disables_and_enables_folder_item() {
        let dir = tempfile::tempdir().unwrap();
        let original_dir = dir.path().join("Startup");
        fs::create_dir_all(&original_dir).unwrap();
        let original = original_dir.join("TestApp.lnk");
        fs::write(&original, b"shortcut").unwrap();

        let store_path = dir.path().join("store.json");
        let quarantine_dir = dir.path().join("quarantine");
        let backup_dir = dir.path().join("backups");

        let entry = StartupEntry {
            title: "TestApp.lnk".to_owned(),
            command: original.display().to_string(),
            source: StartupSource::UserFolder,
            enabled: true,
            kind: StartupEntryKind::Folder {
                path: original.clone(),
            },
        };

        toggle_entry_with_paths(
            &entry,
            &store_path,
            &backup_dir,
            &quarantine_dir,
            std::slice::from_ref(&original_dir),
        )
        .unwrap();
        assert!(!original.exists());
        let store = load_store(&store_path).unwrap();
        assert_eq!(store.entries.len(), 1);

        let disabled = StartupEntry {
            title: store.entries[0].title.clone(),
            command: store.entries[0].command.clone(),
            source: store.entries[0].source,
            enabled: false,
            kind: StartupEntryKind::Disabled(store.entries[0].clone()),
        };

        toggle_entry_with_paths(
            &disabled,
            &store_path,
            &backup_dir,
            &quarantine_dir,
            std::slice::from_ref(&original_dir),
        )
        .unwrap();
        assert!(original.exists());
        assert!(load_store(&store_path).unwrap().entries.is_empty());
    }

    #[test]
    fn refuses_to_restore_folder_outside_startup_roots() {
        let dir = tempfile::tempdir().unwrap();
        let startup = dir.path().join("Startup");
        let outside = dir.path().join("Outside");
        let quarantine = dir.path().join("quarantine");
        fs::create_dir_all(&startup).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&quarantine).unwrap();

        let stored = quarantine.join("item.lnk");
        fs::write(&stored, b"shortcut").unwrap();
        let original = outside.join("item.lnk");
        let store_path = dir.path().join("store.json");
        let disabled = DisabledEntry {
            source: StartupSource::UserFolder,
            title: "item.lnk".to_owned(),
            command: original.display().to_string(),
            disabled_at: now(),
            kind: DisabledKind::Folder {
                original_path: original.clone(),
                stored_path: stored.clone(),
            },
        };
        save_store(
            &store_path,
            &DisabledStore {
                entries: vec![disabled.clone()],
            },
        )
        .unwrap();
        let entry = StartupEntry {
            title: disabled.title.clone(),
            command: disabled.command.clone(),
            source: disabled.source,
            enabled: false,
            kind: StartupEntryKind::Disabled(disabled),
        };

        let result = toggle_entry_with_paths(
            &entry,
            &store_path,
            &dir.path().join("backups"),
            &quarantine,
            std::slice::from_ref(&startup),
        );
        assert!(result.is_err());
        assert!(stored.exists());
        assert!(!original.exists());
        assert_eq!(load_store(&store_path).unwrap().entries.len(), 1);
    }

    #[test]
    fn refuses_to_restore_folder_from_outside_quarantine() {
        let dir = tempfile::tempdir().unwrap();
        let startup = dir.path().join("Startup");
        let quarantine = dir.path().join("quarantine");
        fs::create_dir_all(&startup).unwrap();
        fs::create_dir_all(&quarantine).unwrap();

        let stored = dir.path().join("outside.lnk");
        fs::write(&stored, b"shortcut").unwrap();
        let original = startup.join("item.lnk");
        let store_path = dir.path().join("store.json");
        let disabled = DisabledEntry {
            source: StartupSource::UserFolder,
            title: "item.lnk".to_owned(),
            command: original.display().to_string(),
            disabled_at: now(),
            kind: DisabledKind::Folder {
                original_path: original.clone(),
                stored_path: stored.clone(),
            },
        };
        save_store(
            &store_path,
            &DisabledStore {
                entries: vec![disabled.clone()],
            },
        )
        .unwrap();
        let entry = StartupEntry {
            title: disabled.title.clone(),
            command: disabled.command.clone(),
            source: disabled.source,
            enabled: false,
            kind: StartupEntryKind::Disabled(disabled),
        };

        let result = toggle_entry_with_paths(
            &entry,
            &store_path,
            &dir.path().join("backups"),
            &quarantine,
            std::slice::from_ref(&startup),
        );
        assert!(result.is_err());
        assert!(stored.exists());
        assert!(!original.exists());
        assert_eq!(load_store(&store_path).unwrap().entries.len(), 1);
    }
}
