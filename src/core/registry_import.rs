use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde_json::json;

use crate::{
    config,
    core::{
        journal::Journal,
        reg_file,
        registry_editor::{
            self, EditorHive, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ,
        },
    },
};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_KEYS: usize = 2_000;
const MAX_VALUE_PREVIEWS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportValue {
    pub name: String,
    pub ty: u32,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportKey {
    pub hive: EditorHive,
    pub path: String,
    pub delete: bool,
    pub values: Vec<ImportValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPreview {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub encoding: &'static str,
    pub keys: Vec<ImportKey>,
    pub value_count: usize,
    pub skipped_keys: usize,
    pub preview_truncated: bool,
}

impl ImportPreview {
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn delete_count(&self) -> usize {
        self.keys.iter().filter(|key| key.delete).count()
    }

    pub fn value_previews(&self) -> usize {
        self.keys.iter().map(|key| key.values.len()).sum()
    }
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub backups: usize,
}

pub fn preview(path: &Path) -> Result<ImportPreview> {
    let metadata =
        fs::metadata(path).with_context(|| format!("не удалось прочитать {}", path.display()))?;
    if !metadata.is_file() {
        bail!("Выбранный путь не является файлом");
    }
    if metadata.len() > MAX_FILE_BYTES {
        bail!(
            "Файл больше {} МБ; импорт через интерфейс ограничен",
            MAX_FILE_BYTES / 1024 / 1024
        );
    }

    let bytes =
        fs::read(path).with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let encoding = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        "UTF-16"
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        "UTF-8 BOM"
    } else {
        "UTF-8/ANSI"
    };
    let text = reg_file::decode_text(&bytes);

    let mut keys: Vec<ImportKey> = Vec::new();
    let mut value_count = 0_usize;
    let mut previews = 0_usize;
    let mut skipped_keys = 0_usize;
    let mut preview_truncated = false;

    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            let delete = rest.starts_with('-');
            let key = rest.trim_start_matches('-').trim_matches('\\');
            let Some((hive, path)) = reg_file::parse_key_path(key) else {
                skipped_keys += 1;
                continue;
            };
            if keys.len() >= MAX_KEYS {
                bail!(
                    "Файл затрагивает больше {MAX_KEYS} разделов; импорт через интерфейс ограничен"
                );
            }
            keys.push(ImportKey {
                hive,
                path,
                delete,
                values: Vec::new(),
            });
            continue;
        }

        let Some(current) = keys.last_mut() else {
            continue;
        };
        let Some(value) = parse_value_line(line) else {
            continue;
        };
        value_count += 1;
        if previews < MAX_VALUE_PREVIEWS {
            current.values.push(value);
            previews += 1;
        } else {
            preview_truncated = true;
        }
    }

    if keys.is_empty() {
        if skipped_keys > 0 {
            bail!("В файле нет разделов поддерживаемых веток реестра");
        }
        bail!("Файл не содержит разделов реестра");
    }

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    Ok(ImportPreview {
        path: path.to_path_buf(),
        file_name,
        size: metadata.len(),
        encoding,
        keys,
        value_count,
        skipped_keys,
        preview_truncated,
    })
}

fn parse_value_line(line: &str) -> Option<ImportValue> {
    let (name, data): (String, &str) = if let Some(data) = line.strip_prefix("@=") {
        (String::new(), data.trim())
    } else {
        let rest = line.strip_prefix('"')?;
        let mut escaped = false;
        let mut end = None;
        for (index, ch) in rest.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                end = Some(index);
                break;
            }
        }
        let end = end?;
        let name = reg_file::unescape_name(&rest[..end]);
        let data = rest[end + 1..].trim_start().strip_prefix('=')?.trim();
        (name, data)
    };

    let (ty, preview) = value_preview(data);
    Some(ImportValue { name, ty, preview })
}

fn value_preview(data: &str) -> (u32, String) {
    if data.starts_with('"') {
        let inner = data.trim_start_matches('"').trim_end_matches('"');
        return (REG_SZ, truncate(&reg_file::unescape_name(inner), 90));
    }

    let lower = data.to_ascii_lowercase();
    let ty = if lower.starts_with("dword:") {
        REG_DWORD
    } else if lower.starts_with("hex(b):") {
        REG_QWORD
    } else if lower.starts_with("hex(2):") {
        REG_EXPAND_SZ
    } else if lower.starts_with("hex(7):") {
        REG_MULTI_SZ
    } else {
        REG_BINARY
    };
    (ty, truncate(data, 90))
}

pub fn import(preview: &ImportPreview) -> Result<ImportReport> {
    import_at(preview, &config::backups_dir())
}

fn import_at(preview: &ImportPreview, backups_root: &Path) -> Result<ImportReport> {
    if preview.keys.is_empty() {
        bail!("Предпросмотр не содержит разделов");
    }

    let current = preview_after_check(preview)?;
    if current.keys != preview.keys {
        bail!("Файл изменился после предпросмотра; выберите его заново");
    }

    let stamp = crate::core::unique_stamp();
    let safety_dir = backups_root.join(format!("import-{stamp}"));
    let mut report = ImportReport::default();
    let mut exported = BTreeSet::new();

    for key in &preview.keys {
        if registry_editor::open_key(key.hive, &key.path).is_err() {
            continue;
        }
        let identity = format!("{}\\{}", key.hive.short(), key.path.to_ascii_lowercase());
        if !exported.insert(identity) {
            continue;
        }
        registry_editor::backup_key_to(key.hive, &key.path, &safety_dir)?;
        report.backups += 1;
    }

    apply_keys(&preview.keys)?;

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "reg_imported",
        "file": preview.path.display().to_string(),
        "keys": preview.key_count(),
        "values": preview.value_count,
        "safety_backups": report.backups,
    }));

    Ok(report)
}

fn preview_after_check(preview: &ImportPreview) -> Result<ImportPreview> {
    let current = self::preview(&preview.path)?;
    if current.size != preview.size {
        bail!("Размер файла изменился после предпросмотра; выберите его заново");
    }
    Ok(current)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut value: String = text.chars().take(max).collect();
    value.push('…');
    value
}

fn parse_dword(text: &str) -> Result<u32> {    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("dword:")) {
        return u32::from_str_radix(hex, 16).context("неверный hex-DWORD");
    }
    text.parse::<u32>().context("неверный DWORD")
}

/// Применяет разделы предпросмотра импорта напрямую через windows-registry
/// (без вызова внешней утилиты reg.exe).
fn apply_keys(keys: &[ImportKey]) -> Result<()> {
    for key in keys {
        if key.delete {
            if let Some((parent, child)) = registry_editor::split_parent(&key.path)
                && let Ok(parent_key) = registry_editor::open_key(key.hive, &parent)
            {
                let _ = parent_key.remove_tree(child);
            }
            continue;
        }

        let Ok(key_value) = registry_editor::open_or_create_key(key.hive, &key.path) else {
            continue;
        };
        for value in &key.values {
            if value.ty == REG_DWORD
                && let Ok(number) = parse_dword(&value.preview)
            {
                let _ = key_value.set_u32(&value.name, number);
                continue;
            }
            // QWORD и остальные типы сохраняем строковым представлением.
            let _ = key_value.set_string(&value.name, &value.preview);
        }
    }
    Ok(())
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write registry import journal event");
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
    fn previews_keys_values_and_deletions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.reg");
        fs::write(
            &path,
            concat!(
                "Windows Registry Editor Version 5.00\r\n\r\n",
                "[HKEY_CURRENT_USER\\Software\\ChipClean\\Import]\r\n",
                "\"Name\"=\"hello\"\r\n",
                "\"Number\"=dword:0000002a\r\n",
                "[-HKEY_CURRENT_USER\\Software\\ChipClean\\Gone]\r\n",
            ),
        )
        .unwrap();

        let preview = preview(&path).unwrap();

        assert_eq!(preview.key_count(), 2);
        assert_eq!(preview.value_count, 2);
        assert_eq!(preview.delete_count(), 1);
        assert_eq!(preview.keys[0].hive, EditorHive::Hkcu);
        assert_eq!(preview.keys[0].path, r"Software\ChipClean\Import");
        assert_eq!(preview.keys[0].values[0].name, "Name");
        assert_eq!(preview.keys[0].values[0].preview, "hello");
        assert_eq!(preview.keys[0].values[1].ty, REG_DWORD);
    }

    #[test]
    fn imports_a_previewed_file() {
        let _guard = crate::core::registry_test_guard();
        use windows_registry::CURRENT_USER;

        let unique = format!(
            "ChipCleanImport-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let key_path = format!(r"Software\{unique}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("import.reg");
        fs::write(
            &path,
            format!(
                "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\{key_path}]\r\n\"Value\"=\"imported\"\r\n"
            ),
        )
        .unwrap();

        let preview = preview(&path).unwrap();
        let report = import_at(&preview, dir.path()).unwrap();

        assert_eq!(report.backups, 0);
        let key = CURRENT_USER.open(&key_path).unwrap();
        assert_eq!(key.get_string("Value").unwrap(), "imported");
        drop(key);
        let _ = CURRENT_USER.remove_tree(&key_path);
    }
}
