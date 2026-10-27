use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde_json::json;
use windows_registry::{CLASSES_ROOT, CURRENT_CONFIG, CURRENT_USER, Key, LOCAL_MACHINE, USERS};

use crate::core::journal::Journal;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const KEY_STATS_LIMIT: usize = 5_000;

pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;
pub const REG_MULTI_SZ: u32 = 7;
pub const REG_QWORD: u32 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorHive {
    Hkcu,
    Hklm,
    Hkcr,
    Hku,
    Hkcc,
}

impl EditorHive {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hkcu => "HKEY_CURRENT_USER",
            Self::Hklm => "HKEY_LOCAL_MACHINE",
            Self::Hkcr => "HKEY_CLASSES_ROOT",
            Self::Hku => "HKEY_USERS",
            Self::Hkcc => "HKEY_CURRENT_CONFIG",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Hkcu => "HKCU",
            Self::Hklm => "HKLM",
            Self::Hkcr => "HKCR",
            Self::Hku => "HKU",
            Self::Hkcc => "HKCC",
        }
    }

    pub fn reg_name(self) -> &'static str {
        match self {
            Self::Hkcu => "HKEY_CURRENT_USER",
            Self::Hklm => "HKEY_LOCAL_MACHINE",
            Self::Hkcr => "HKEY_CLASSES_ROOT",
            Self::Hku => "HKEY_USERS",
            Self::Hkcc => "HKEY_CURRENT_CONFIG",
        }
    }

    fn root(self) -> &'static Key {
        match self {
            Self::Hkcu => CURRENT_USER,
            Self::Hklm => LOCAL_MACHINE,
            Self::Hkcr => CLASSES_ROOT,
            Self::Hku => USERS,
            Self::Hkcc => CURRENT_CONFIG,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegValueRow {
    pub name: String,
    pub ty: u32,
    pub preview: String,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: String,
    pub value_name: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyStats {
    pub subkeys: usize,
    pub values: usize,
    pub truncated: bool,
}

pub fn open_key(hive: EditorHive, path: &str) -> Result<Key> {
    hive.root()
        .open(path)
        .with_context(|| format!("не удалось открыть {}", full_path(hive, path)))
}

fn open_key_rw(hive: EditorHive, path: &str) -> Result<Key> {
    hive.root()
        .options()
        .read()
        .write()
        .open(path)
        .with_context(|| format!("не удалось открыть для записи {}", full_path(hive, path)))
}

pub fn list_subkeys(hive: EditorHive, path: &str) -> Result<Vec<String>> {
    let key = open_key(hive, path)?;
    let mut names: Vec<String> = key.keys()?.collect();
    names.sort_by_key(|name| name.to_lowercase());
    Ok(names)
}

pub fn list_values(hive: EditorHive, path: &str) -> Result<Vec<RegValueRow>> {
    let key = open_key(hive, path)?;
    let mut rows = Vec::new();
    for (name, value) in key.values()? {
        rows.push(RegValueRow {
            name,
            ty: u32::from(value.ty()),
            preview: preview_value(&value),
            size: value.len(),
        });
    }
    rows.sort_by_key(|row| row.name.to_lowercase());
    Ok(rows)
}

pub fn read_value_text(hive: EditorHive, path: &str, name: &str) -> Result<(u32, String)> {
    let key = open_key(hive, path)?;
    let value = key
        .get_value(name)
        .with_context(|| format!("не удалось прочитать значение {}", display_value_name(name)))?;
    let ty = u32::from(value.ty());
    Ok((ty, value_to_text(&value)))
}

pub fn write_value(
    hive: EditorHive,
    path: &str,
    name: &str,
    ty: u32,
    text: &str,
    backup_dir: &Path,
) -> Result<PathBuf> {
    let key = open_key_rw(hive, path)?;
    let backup = backup_key_to(hive, path, backup_dir)?;
    write_raw(&key, name, ty, text)?;
    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "registry_value_set",
        "key": full_path(hive, path),
        "value": name,
        "type": ty,
        "backup": backup.display().to_string(),
    }));
    Ok(backup)
}

pub fn delete_value(
    hive: EditorHive,
    path: &str,
    name: &str,
    backup_dir: &Path,
) -> Result<PathBuf> {
    let key = open_key_rw(hive, path)?;
    let backup = backup_key_to(hive, path, backup_dir)?;
    key.remove_value(name)
        .with_context(|| format!("не удалось удалить значение {}", display_value_name(name)))?;
    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "registry_value_deleted",
        "key": full_path(hive, path),
        "value": name,
        "backup": backup.display().to_string(),
    }));
    Ok(backup)
}

pub(crate) fn remove_value_raw(hive: EditorHive, path: &str, name: &str) -> Result<()> {
    let key = open_key_rw(hive, path)?;
    key.remove_value(name)
        .with_context(|| format!("не удалось удалить значение {}", display_value_name(name)))?;
    Ok(())
}

pub fn create_key(
    hive: EditorHive,
    parent_path: &str,
    name: &str,
    backup_dir: &Path,
) -> Result<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('\\') {
        bail!("Имя раздела не должно быть пустым или содержать \\");
    }
    let parent = open_key_rw(hive, parent_path)?;
    if parent
        .keys()?
        .any(|existing| existing.eq_ignore_ascii_case(name))
    {
        bail!("Раздел уже существует");
    }
    let backup = backup_key_to(hive, parent_path, backup_dir)?;
    parent
        .create(name)
        .with_context(|| format!("не удалось создать раздел {name}"))?;
    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "registry_key_created",
        "key": full_path(hive, &join_path(parent_path, name)),
        "backup": backup.display().to_string(),
    }));
    Ok(backup)
}

pub fn key_stats(hive: EditorHive, path: &str, limit: usize) -> Result<KeyStats> {
    let cap = limit.max(1);
    let mut stats = KeyStats::default();
    let mut queue = VecDeque::from([path.to_owned()]);

    while let Some(current) = queue.pop_front() {
        let Ok(key) = open_key(hive, &current) else {
            continue;
        };
        if let Ok(values) = key.values() {
            stats.values = stats.values.saturating_add(values.count());
        }
        if stats.subkeys.saturating_add(stats.values) >= cap {
            stats.truncated = true;
            return Ok(stats);
        }
        if let Ok(keys) = key.keys() {
            for child in keys {
                stats.subkeys = stats.subkeys.saturating_add(1);
                queue.push_back(join_path(&current, &child));
                if stats.subkeys.saturating_add(stats.values) >= cap {
                    stats.truncated = true;
                    return Ok(stats);
                }
            }
        }
    }

    Ok(stats)
}

pub fn delete_key(hive: EditorHive, path: &str, backup_dir: &Path) -> Result<PathBuf> {
    let (parent_path, name) =
        split_parent(path).ok_or_else(|| anyhow!("Нельзя удалить корневой раздел."))?;
    let parent = open_key_rw(hive, &parent_path)?;
    let backup = backup_key_to(hive, path, backup_dir)?;
    parent
        .remove_tree(&name)
        .with_context(|| format!("не удалось удалить раздел {name}"))?;
    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "registry_key_deleted",
        "key": full_path(hive, path),
        "backup": backup.display().to_string(),
    }));
    Ok(backup)
}

pub fn search_cancellable(
    hive: EditorHive,
    root: &str,
    query: &str,
    limit: usize,
    cancel: &AtomicBool,
) -> Result<Vec<SearchHit>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let cap = limit.clamp(1, 500);
    let mut queue = VecDeque::from([root.to_owned()]);
    let mut hits = Vec::new();
    let mut visited = 0_usize;

    while let Some(path) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        visited += 1;
        if visited > 20_000 || hits.len() >= cap {
            break;
        }

        let Ok(key) = open_key(hive, &path) else {
            continue;
        };

        if (path.to_lowercase().contains(&query) || hive.short().to_lowercase().contains(&query))
            && !path.is_empty()
        {
            hits.push(SearchHit {
                path: path.clone(),
                value_name: None,
                detail: "Раздел".to_owned(),
            });
        }

        if let Ok(values) = key.values() {
            for (name, value) in values {
                if hits.len() >= cap {
                    break;
                }
                let preview = preview_value(&value);
                if name.to_lowercase().contains(&query) || preview.to_lowercase().contains(&query) {
                    hits.push(SearchHit {
                        path: path.clone(),
                        value_name: Some(name),
                        detail: preview,
                    });
                }
            }
        }

        if let Ok(keys) = key.keys() {
            for child in keys {
                if matches!(path.as_str(), "" | "Software") && child.eq_ignore_ascii_case("Classes")
                {
                    continue;
                }
                queue.push_back(join_path(&path, &child));
                if queue.len() > 5000 {
                    break;
                }
            }
        }
    }

    hits.sort_by(|left, right| {
        left.path
            .to_lowercase()
            .cmp(&right.path.to_lowercase())
            .then_with(|| left.value_name.cmp(&right.value_name))
    });
    Ok(hits)
}

#[derive(Debug, Clone)]
pub struct ReplaceHit {
    pub hive: EditorHive,
    pub path: String,
    pub value_name: String,
    pub ty: u32,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplaceReport {
    pub replaced: usize,
    pub backups: usize,
}

pub fn find_replacements_cancellable(
    hive: EditorHive,
    root: &str,
    find: &str,
    replacement: &str,
    limit: usize,
    cancel: &AtomicBool,
) -> Result<Vec<ReplaceHit>> {
    let find = find.trim();
    if find.is_empty() {
        return Ok(Vec::new());
    }
    let cap = limit.clamp(1, 1_000);
    let mut queue = VecDeque::from([root.to_owned()]);
    let mut hits = Vec::new();
    let mut visited = 0_usize;

    while let Some(path) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        visited += 1;
        if visited > 20_000 || hits.len() >= cap {
            break;
        }

        let Ok(key) = open_key(hive, &path) else {
            continue;
        };

        if let Ok(values) = key.values() {
            for (name, value) in values {
                if hits.len() >= cap {
                    break;
                }
                let ty = u32::from(value.ty());
                if !matches!(ty, REG_SZ | REG_EXPAND_SZ | REG_MULTI_SZ) {
                    continue;
                }
                let text = value_to_text(&value);
                let Some(after) = replace_case_insensitive(&text, find, replacement) else {
                    continue;
                };
                if after == text {
                    continue;
                }
                hits.push(ReplaceHit {
                    hive,
                    path: path.clone(),
                    value_name: name,
                    ty,
                    before: text,
                    after,
                });
            }
        }

        if let Ok(keys) = key.keys() {
            for child in keys {
                if matches!(path.as_str(), "" | "Software") && child.eq_ignore_ascii_case("Classes")
                {
                    continue;
                }
                queue.push_back(join_path(&path, &child));
                if queue.len() > 5000 {
                    break;
                }
            }
        }
    }

    hits.sort_by(|left, right| {
        left.path
            .to_lowercase()
            .cmp(&right.path.to_lowercase())
            .then_with(|| left.value_name.cmp(&right.value_name))
    });
    Ok(hits)
}

pub fn replace_values(hits: &[ReplaceHit], backup_dir: &Path) -> Result<ReplaceReport> {
    if hits.is_empty() {
        bail!("Нет значений для замены");
    }

    for hit in hits {
        let (ty, text) = read_value_text(hit.hive, &hit.path, &hit.value_name)?;
        if ty != hit.ty || text != hit.before {
            bail!(
                "Значение {} изменилось после поиска; повторите поиск и замену",
                full_path(hit.hive, &hit.path)
            );
        }
    }

    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let safety_dir = backup_dir.join(format!("replace-{stamp}"));
    let mut backed_up = BTreeSet::new();
    let mut report = ReplaceReport::default();

    for hit in hits {
        let identity = format!("{}\\{}", hit.hive.short(), hit.path.to_ascii_lowercase());
        if backed_up.insert(identity) {
            backup_key_to(hit.hive, &hit.path, &safety_dir)?;
            report.backups += 1;
        }
    }

    for hit in hits {
        let key = open_key_rw(hit.hive, &hit.path)?;
        write_raw(&key, &hit.value_name, hit.ty, &hit.after)?;
        report.replaced += 1;
    }

    journal(json!({
        "ts": Utc::now().to_rfc3339(),
        "event": "registry_value_replaced",
        "keys": report.backups,
        "values": report.replaced,
        "backup": safety_dir.display().to_string(),
    }));

    Ok(report)
}

fn replace_case_insensitive(text: &str, find: &str, replacement: &str) -> Option<String> {
    let needle: Vec<char> = find.chars().collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if needle.is_empty() || chars.len() < needle.len() {
        return None;
    }

    let mut output = String::with_capacity(text.len());
    let mut index = 0_usize;
    let mut changed = false;
    while index < chars.len() {
        let matches = index + needle.len() <= chars.len()
            && needle
                .iter()
                .enumerate()
                .all(|(offset, expected)| char_eq_ignore_case(chars[index + offset].1, *expected));
        if matches {
            output.push_str(replacement);
            index += needle.len();
            changed = true;
        } else {
            output.push(chars[index].1);
            index += 1;
        }
    }
    changed.then_some(output)
}

fn char_eq_ignore_case(left: char, right: char) -> bool {
    left == right || left.to_lowercase().eq(right.to_lowercase())
}

pub fn full_path(hive: EditorHive, path: &str) -> String {
    if path.trim().is_empty() {
        hive.short().to_owned()
    } else {
        format!("{}\\{}", hive.short(), path)
    }
}

pub fn display_value_name(name: &str) -> &str {
    if name.is_empty() {
        "(По умолчанию)"
    } else {
        name
    }
}

pub fn type_label(ty: u32) -> &'static str {
    match ty {
        REG_SZ => "REG_SZ",
        REG_EXPAND_SZ => "REG_EXPAND_SZ",
        REG_BINARY => "REG_BINARY",
        REG_DWORD => "REG_DWORD",
        REG_MULTI_SZ => "REG_MULTI_SZ",
        REG_QWORD => "REG_QWORD",
        _ => "REG_UNKNOWN",
    }
}

pub fn preview_value(value: &windows_registry::Value) -> String {
    let text = value_to_text(value);
    let text = text.replace(['\r', '\n'], " ");
    truncate(&text, 96)
}

pub fn value_to_text(value: &windows_registry::Value) -> String {
    let ty = u32::from(value.ty());
    match ty {
        REG_SZ | REG_EXPAND_SZ => {
            let wide = value.as_wide();
            let end = wide.iter().position(|ch| *ch == 0).unwrap_or(wide.len());
            String::from_utf16_lossy(&wide[..end])
        }
        REG_DWORD => value
            .get(0..4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap_or_default()).to_string())
            .unwrap_or_default(),
        REG_QWORD => value
            .get(0..8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap_or_default()).to_string())
            .unwrap_or_default(),
        REG_MULTI_SZ => {
            let wide = value.as_wide();
            let mut parts = Vec::new();
            let mut start = 0;
            for (index, ch) in wide.iter().enumerate() {
                if *ch == 0 {
                    if index > start {
                        parts.push(String::from_utf16_lossy(&wide[start..index]));
                    }
                    start = index + 1;
                }
            }
            parts.join("\n")
        }
        _ => {
            let mut output = String::new();
            for (index, byte) in value.iter().enumerate() {
                if index > 0 && index % 2 == 0 {
                    output.push(' ');
                }
                output.push_str(&format!("{byte:02X}"));
                if output.len() > 180 {
                    output.push('…');
                    break;
                }
            }
            output
        }
    }
}

pub fn write_raw(key: &Key, name: &str, ty: u32, text: &str) -> Result<()> {
    match ty {
        REG_SZ => key.set_string(name, text)?,
        REG_EXPAND_SZ => key.set_expand_string(name, text)?,
        REG_DWORD => {
            let value = parse_u64(text)?;
            let value = u32::try_from(value).context("REG_DWORD вне диапазона u32")?;
            key.set_u32(name, value)?;
        }
        REG_QWORD => {
            let value = parse_u64(text)?;
            key.set_u64(name, value)?;
        }
        REG_MULTI_SZ => {
            let lines: Vec<&str> = text
                .lines()
                .map(|line| line.trim_end_matches('\r'))
                .collect();
            key.set_multi_string(name, &lines)?;
        }
        REG_BINARY => {
            let bytes = parse_hex(text)?;
            key.set_bytes(name, windows_registry::Type::Bytes, &bytes)?;
        }
        other => {
            let bytes = parse_hex(text)?;
            key.set_bytes(name, windows_registry::Type::Other(other), &bytes)?;
        }
    }
    Ok(())
}

pub fn backup_key_to(hive: EditorHive, path: &str, base_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(base_dir)
        .with_context(|| format!("не удалось создать {}", base_dir.display()))?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let label = if path.trim().is_empty() {
        "root".to_owned()
    } else {
        path.replace('\\', "_")
    };
    let path_file = base_dir.join(format!("editor-{stamp}-{}.reg", sanitize(&label)));
    let full = if path.trim().is_empty() {
        hive.reg_name().to_owned()
    } else {
        format!("{}\\{}", hive.reg_name(), path)
    };
    let output = Command::new(reg_exe())
        .creation_flags(CREATE_NO_WINDOW)
        .arg("export")
        .arg(&full)
        .arg(&path_file)
        .arg("/y")
        .output()
        .with_context(|| format!("не удалось запустить reg.exe для {full}"))?;
    if !output.status.success() {
        bail!(
            "reg.exe export завершился с {} для {full}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if !path_file.is_file() {
        bail!("reg.exe не создал файл {}", path_file.display());
    }
    Ok(path_file)
}

pub fn import_reg_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("Файл .reg не найден: {}", path.display());
    }
    let output = Command::new(reg_exe())
        .creation_flags(CREATE_NO_WINDOW)
        .arg("import")
        .arg(path)
        .output()
        .with_context(|| format!("не удалось запустить reg.exe для {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "reg.exe import завершился с {} для {}: {}",
            output.status,
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub fn split_parent(path: &str) -> Option<(String, String)> {
    let trimmed = path.trim_matches('\\');
    let (parent, name) = trimmed.rsplit_once('\\')?;
    Some((parent.to_owned(), name.to_owned()))
}

pub fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}\\{name}")
    }
}

fn parse_u64(text: &str) -> Result<u64> {
    let text = text.trim();
    let value = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        text.parse::<u64>()
    };
    value.context("ожидается число")
}

fn parse_hex(text: &str) -> Result<Vec<u8>> {
    let cleaned: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != ',' && *ch != '-' && *ch != ':')
        .collect();
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    if !cleaned.len().is_multiple_of(2) {
        bail!("hex-значение должно содержать чётное число символов");
    }
    let mut bytes = Vec::with_capacity(cleaned.len() / 2);
    let chars: Vec<char> = cleaned.chars().collect();
    for chunk in chars.chunks(2) {
        let pair: String = chunk.iter().collect();
        bytes.push(u8::from_str_radix(&pair, 16).context("некорректный hex")?);
    }
    Ok(bytes)
}

fn sanitize(path: &str) -> String {
    let mut output: String = path
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .take(80)
        .collect();
    if output.is_empty() {
        output.push_str("key");
    }
    output
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut value: String = text.chars().take(max).collect();
    value.push('…');
    value
}

fn reg_exe() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("reg.exe")
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write registry editor journal event");
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
    use windows_registry::CURRENT_USER;

    #[test]
    fn replaces_text_values_after_backup() {
        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanReplace-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let parent = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&parent).unwrap();
        write_raw(&key, "Text", REG_SZ, "Hello world, hello").unwrap();
        write_raw(&key, "Number", REG_DWORD, "42").unwrap();
        drop(key);

        let cancel = AtomicBool::new(false);
        let hits =
            find_replacements_cancellable(EditorHive::Hkcu, &parent, "HELLO", "Hi", 10, &cancel)
                .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value_name, "Text");
        assert_eq!(hits[0].after, "Hi world, Hi");

        let dir = tempfile::tempdir().unwrap();
        let report = replace_values(&hits, dir.path()).unwrap();
        assert_eq!(report.replaced, 1);
        assert_eq!(report.backups, 1);

        let (_, text) = read_value_text(EditorHive::Hkcu, &parent, "Text").unwrap();
        assert_eq!(text, "Hi world, Hi");
        let (_, number) = read_value_text(EditorHive::Hkcu, &parent, "Number").unwrap();
        assert_eq!(number, "42");

        let _ = CURRENT_USER.remove_tree(&parent);
    }

    #[test]
    fn replace_is_case_insensitive() {
        assert_eq!(
            replace_case_insensitive("abc ABC", "abc", "x").as_deref(),
            Some("x x")
        );
        assert_eq!(
            replace_case_insensitive("Hello world", "WORLD", "there").as_deref(),
            Some("Hello there")
        );
        assert!(replace_case_insensitive("abc", "abcd", "x").is_none());
        assert!(replace_case_insensitive("abc", "", "x").is_none());
    }

    #[test]
    fn parses_hex_values() {
        assert_eq!(parse_hex("01 02 ff").unwrap(), vec![1, 2, 255]);
        assert!(parse_hex("1").is_err());
    }

    #[test]
    fn writes_and_deletes_test_values() {
        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanEditor-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let parent = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&parent).unwrap();
        let dir = tempfile::tempdir().unwrap();

        write_raw(&key, "Text", REG_SZ, "hello").unwrap();
        let (ty, text) = read_value_text(EditorHive::Hkcu, &parent, "Text").unwrap();
        assert_eq!(ty, REG_SZ);
        assert_eq!(text, "hello");

        write_raw(&key, "Number", REG_DWORD, "42").unwrap();
        let (_, text) = read_value_text(EditorHive::Hkcu, &parent, "Number").unwrap();
        assert_eq!(text, "42");

        let backup = delete_value(EditorHive::Hkcu, &parent, "Text", dir.path()).unwrap();
        assert!(backup.is_file());
        assert!(key.get_value("Text").is_err());

        drop(key);
        let _ = CURRENT_USER.remove_tree(&parent);
    }

    #[test]
    fn creates_and_removes_test_key() {
        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanEditorKey-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let parent = format!(r"Software\{unique}");
        CURRENT_USER.create(&parent).unwrap();
        let dir = tempfile::tempdir().unwrap();

        let create_backup = create_key(EditorHive::Hkcu, &parent, "Child", dir.path()).unwrap();
        assert!(create_backup.is_file());

        let child_path = format!(r"{parent}\Child");
        let child = open_key_rw(EditorHive::Hkcu, &child_path).unwrap();
        write_raw(&child, "Value", REG_SZ, "hello").unwrap();
        child.create("Grand").unwrap();
        drop(child);

        let stats = key_stats(EditorHive::Hkcu, &child_path, 100).unwrap();
        assert_eq!(stats.subkeys, 1);
        assert_eq!(stats.values, 1);
        assert!(!stats.truncated);

        let limited = key_stats(EditorHive::Hkcu, &child_path, 1).unwrap();
        assert!(limited.truncated);

        let backup = delete_key(EditorHive::Hkcu, &child_path, dir.path()).unwrap();
        assert!(backup.is_file());
        assert!(CURRENT_USER.open(&child_path).is_err());

        let _ = CURRENT_USER.remove_tree(&parent);
    }
}
