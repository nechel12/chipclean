use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    config,
    core::{
        fs_atomic, reg_file,
        registry_editor::{EditorHive, full_path, join_path, list_values, open_key},
    },
};

const SNAPSHOT_KEY_LIMIT: usize = 120_000;
const DIFF_LIMIT: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRoot {
    pub hive: String,
    pub path: String,
    pub label: String,
}

impl SnapshotRoot {
    pub fn display(&self) -> String {
        if self.path.trim().is_empty() {
            self.hive.clone()
        } else {
            format!("{}\\{}", self.hive, self.path)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub roots: Vec<SnapshotRoot>,
    pub file: String,
    pub keys: usize,
    pub values: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotValue {
    pub name: String,
    pub ty: u32,
    pub hash: u64,
    pub size: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotKey {
    pub path: String,
    pub values: Vec<SnapshotValue>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SnapshotIndex {
    entries: Vec<SnapshotMeta>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SnapshotData {
    entries: Vec<SnapshotKey>,
    truncated: bool,
}

#[derive(Debug, Clone)]
pub struct ValueDiff {
    pub path: String,
    pub name: String,
    pub before: Option<SnapshotValue>,
    pub after: Option<SnapshotValue>,
}

#[derive(Debug, Default, Clone)]
pub struct SnapshotDiff {
    pub added_keys: Vec<String>,
    pub removed_keys: Vec<String>,
    pub added_values: Vec<ValueDiff>,
    pub removed_values: Vec<ValueDiff>,
    pub changed_values: Vec<ValueDiff>,
    pub truncated: bool,
    pub lists_truncated: bool,
}

impl SnapshotDiff {
    pub fn is_empty(&self) -> bool {
        self.added_keys.is_empty()
            && self.removed_keys.is_empty()
            && self.added_values.is_empty()
            && self.removed_values.is_empty()
            && self.changed_values.is_empty()
    }
}

pub fn root_options() -> Vec<SnapshotRoot> {
    vec![
        SnapshotRoot {
            hive: "HKCU".to_owned(),
            path: r"Software".to_owned(),
            label: "Настройки пользователя".to_owned(),
        },
        SnapshotRoot {
            hive: "HKLM".to_owned(),
            path: r"Software".to_owned(),
            label: "Настройки программ".to_owned(),
        },
        SnapshotRoot {
            hive: "HKLM".to_owned(),
            path: r"System\CurrentControlSet\Services".to_owned(),
            label: "Службы".to_owned(),
        },
    ]
}

pub fn list_snapshots() -> Result<Vec<SnapshotMeta>> {
    let path = index_path();
    let mut index = load_index(&path)?;
    index
        .entries
        .sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(index.entries)
}

fn snapshot_file_name(meta: &SnapshotMeta) -> Result<String> {
    let id = meta.id.trim();
    let valid_id = !id.is_empty()
        && id.len() <= 128
        && id.starts_with("snapshot-")
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
    if !valid_id {
        bail!("некорректный идентификатор снимка");
    }

    let expected = format!("{id}.json");
    if meta.file != expected {
        bail!("некорректное имя файла снимка");
    }
    Ok(expected)
}

fn snapshot_path(meta: &SnapshotMeta) -> Result<PathBuf> {
    let name = snapshot_file_name(meta)?;
    let dir = config::snapshots_dir();
    let path = dir.join(name);

    if path.exists() {
        let canonical_dir = fs::canonicalize(&dir)
            .with_context(|| format!("не удалось проверить {}", dir.display()))?;
        let canonical_path = fs::canonicalize(&path)
            .with_context(|| format!("не удалось проверить {}", path.display()))?;
        if !canonical_path.starts_with(&canonical_dir) {
            bail!("файл снимка находится вне каталога снимков");
        }
    }

    Ok(path)
}

pub fn create_snapshot(
    name: &str,
    roots: &[SnapshotRoot],
    cancel: &AtomicBool,
) -> Result<SnapshotMeta> {
    if roots.is_empty() {
        bail!("Выберите хотя бы один раздел");
    }
    let name = name.trim();
    let name = if name.is_empty() {
        format!("Снимок {}", Utc::now().format("%d.%m.%Y %H:%M"))
    } else {
        name.to_owned()
    };

    let data = capture(roots, cancel)?;
    let id = format!("snapshot-{}", crate::core::unique_stamp());
    let file = format!("{id}.json");
    let dir = config::snapshots_dir();
    fs::create_dir_all(&dir).with_context(|| format!("не удалось создать {}", dir.display()))?;
    let path = dir.join(&file);
    fs_atomic::write_json_atomic_compact(&path, &data)?;

    let meta = SnapshotMeta {
        id,
        name,
        created_at: Utc::now().to_rfc3339(),
        roots: roots.to_vec(),
        file,
        keys: data.entries.len(),
        values: data.entries.iter().map(|key| key.values.len()).sum(),
        truncated: data.truncated,
    };

    let mut index = load_index(&index_path())?;
    index.entries.push(meta.clone());
    if let Err(err) = save_index(&index) {
        let _ = fs::remove_file(&path);
        return Err(err);
    }
    Ok(meta)
}

pub fn delete_snapshot(meta: &SnapshotMeta) -> Result<()> {
    let path = snapshot_path(meta)?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("не удалось удалить {}", path.display()))?;
    }
    let mut index = load_index(&index_path())?;
    index.entries.retain(|entry| entry.id != meta.id);
    save_index(&index)
}

pub fn compare(meta: &SnapshotMeta, cancel: &AtomicBool) -> Result<SnapshotDiff> {
    let path = snapshot_path(meta)?;
    let raw =
        fs::read(&path).with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let before: SnapshotData = serde_json::from_slice(&raw)
        .with_context(|| format!("не удалось разобрать {}", path.display()))?;
    let after_data = capture(&meta.roots, cancel)?;
    let after_truncated = after_data.truncated;

    let before = key_map(before.entries);
    let after = key_map(after_data.entries);
    let mut diff = SnapshotDiff {
        truncated: meta.truncated || after_truncated,
        ..SnapshotDiff::default()
    };

    for path in after.keys() {
        if !before.contains_key(path) && diff.added_keys.len() < DIFF_LIMIT {
            diff.added_keys.push(path.clone());
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) && diff.removed_keys.len() < DIFF_LIMIT {
            diff.removed_keys.push(path.clone());
        }
    }

    for (path, after_values) in &after {
        let Some(before_values) = before.get(path) else {
            continue;
        };
        for (name, after_value) in after_values {
            match before_values.get(name) {
                None if diff.added_values.len() < DIFF_LIMIT => {
                    diff.added_values.push(ValueDiff {
                        path: path.clone(),
                        name: name.clone(),
                        before: None,
                        after: Some(after_value.clone()),
                    });
                }
                Some(before_value)
                    if before_value.hash != after_value.hash
                        && diff.changed_values.len() < DIFF_LIMIT =>
                {
                    diff.changed_values.push(ValueDiff {
                        path: path.clone(),
                        name: name.clone(),
                        before: Some(before_value.clone()),
                        after: Some(after_value.clone()),
                    });
                }
                _ => {}
            }
        }
        for (name, before_value) in before_values {
            if !after_values.contains_key(name) && diff.removed_values.len() < DIFF_LIMIT {
                diff.removed_values.push(ValueDiff {
                    path: path.clone(),
                    name: name.clone(),
                    before: Some(before_value.clone()),
                    after: None,
                });
            }
        }
    }

    diff.lists_truncated = diff.added_keys.len() >= DIFF_LIMIT
        || diff.removed_keys.len() >= DIFF_LIMIT
        || diff.added_values.len() >= DIFF_LIMIT
        || diff.removed_values.len() >= DIFF_LIMIT
        || diff.changed_values.len() >= DIFF_LIMIT;
    diff.added_keys.sort();
    diff.removed_keys.sort();
    Ok(diff)
}

fn capture(roots: &[SnapshotRoot], cancel: &AtomicBool) -> Result<SnapshotData> {
    let mut entries = Vec::new();
    let mut total = 0_usize;
    let mut truncated = false;

    for root in roots {
        let hive = parse_hive(&root.hive)?;
        let root_path = root.path.trim_matches('\\').to_owned();
        let mut queue = VecDeque::from([root_path.clone()]);

        while let Some(path) = queue.pop_front() {
            if cancel.load(Ordering::Relaxed) {
                bail!("Операция отменена");
            }
            if total >= SNAPSHOT_KEY_LIMIT {
                truncated = true;
                break;
            }
            let Ok(key) = open_key(hive, &path) else {
                continue;
            };
            let mut values = Vec::new();
            if let Ok(rows) = list_values(hive, &path) {
                for row in rows {
                    let Ok(value) = key.get_value(&row.name) else {
                        continue;
                    };
                    values.push(SnapshotValue {
                        name: row.name,
                        ty: row.ty,
                        hash: hash_bytes(row.ty, value.as_ref()),
                        size: value.len(),
                        preview: row.preview,
                    });
                }
            }
            values.sort_by(|left, right| left.name.cmp(&right.name));
            entries.push(SnapshotKey {
                path: full_path(hive, &path),
                values,
            });
            total += 1;

            if let Ok(keys) = key.keys() {
                for child in keys {
                    queue.push_back(join_path(&path, &child));
                }
            }
        }
        if truncated {
            break;
        }
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries.dedup_by(|left, right| left.path == right.path);
    Ok(SnapshotData { entries, truncated })
}

fn key_map(entries: Vec<SnapshotKey>) -> HashMap<String, BTreeMap<String, SnapshotValue>> {
    entries
        .into_iter()
        .map(|key| {
            (
                key.path,
                key.values
                    .into_iter()
                    .map(|value| (value.name.clone(), value))
                    .collect(),
            )
        })
        .collect()
}

fn parse_hive(value: &str) -> Result<EditorHive> {
    reg_file::parse_hive(value).ok_or_else(|| anyhow!("неизвестная ветка снимка: {value}"))
}

fn hash_bytes(ty: u32, bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in ty.to_le_bytes().iter().chain(bytes) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn index_path() -> PathBuf {
    config::snapshots_dir().join("index.json")
}

fn load_index(path: &Path) -> Result<SnapshotIndex> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(SnapshotIndex::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось прочитать {}", path.display()));
        }
    };
    serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .with_context(|| format!("не удалось разобрать {}", path.display()))
}

fn save_index(index: &SnapshotIndex) -> Result<()> {
    let dir = config::snapshots_dir();
    fs::create_dir_all(&dir).with_context(|| format!("не удалось создать {}", dir.display()))?;
    fs_atomic::write_json_atomic(&index_path(), index)
}

pub fn snapshot_size(meta: &SnapshotMeta) -> Result<u64> {
    let path = snapshot_path(meta)?;
    fs::metadata(&path)
        .map(|metadata| metadata.len())
        .map_err(|err| anyhow!("не удалось прочитать {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry_editor::{REG_DWORD, REG_SZ};
    use windows_registry::CURRENT_USER;

    #[test]
    fn hash_changes_with_type() {
        assert_ne!(hash_bytes(REG_SZ, b"a"), hash_bytes(REG_SZ, b"b"));
        assert_ne!(hash_bytes(REG_SZ, b"a"), hash_bytes(REG_DWORD, b"a"));
    }

    #[test]
    fn parses_every_supported_hive() {
        assert_eq!(parse_hive("hkcu").unwrap(), EditorHive::Hkcu);
        assert_eq!(parse_hive("HKLM").unwrap(), EditorHive::Hklm);
        assert_eq!(parse_hive("HKCR").unwrap(), EditorHive::Hkcr);
        assert_eq!(parse_hive("HKEY_USERS").unwrap(), EditorHive::Hku);
        assert_eq!(parse_hive("HKCC").unwrap(), EditorHive::Hkcc);
        assert!(parse_hive("HKXX").is_err());
    }

    #[test]
    fn displays_a_full_hive_without_a_trailing_slash() {
        let root = SnapshotRoot {
            hive: "HKLM".to_owned(),
            path: String::new(),
            label: "Весь улей".to_owned(),
        };
        assert_eq!(root.display(), "HKLM");
    }

    fn meta_with(id: &str, file: &str) -> SnapshotMeta {
        SnapshotMeta {
            id: id.to_owned(),
            name: "test".to_owned(),
            created_at: Utc::now().to_rfc3339(),
            roots: Vec::new(),
            file: file.to_owned(),
            keys: 0,
            values: 0,
            truncated: false,
        }
    }

    #[test]
    fn validates_snapshot_file_names() {
        let valid = meta_with("snapshot-ok", "snapshot-ok.json");
        assert_eq!(snapshot_file_name(&valid).unwrap(), "snapshot-ok.json");

        let invalid = [
            ("../snapshot-ok", "../snapshot-ok.json"),
            ("snapshot-ok", r"C:\evil.json"),
            ("snapshot-ok", "other.json"),
            ("index", "index.json"),
            ("snapshot-ok", "snapshot-ok.json:stream"),
            ("snapshot-ok", r"snapshot-ok.json\..\evil.json"),
        ];
        for (id, file) in invalid {
            let meta = meta_with(id, file);
            assert!(
                snapshot_file_name(&meta).is_err(),
                "должно быть отклонено: id={id:?}, file={file:?}"
            );
        }
    }

    #[test]
    fn captures_a_small_root() {
        let _guard = crate::core::registry_test_guard();
        let unique = format!(
            "ChipCleanSnapshot-{}-{}",
            std::process::id(),
            Utc::now().timestamp_micros()
        );
        let path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&path).unwrap();
        key.set_string("Test", "value").unwrap();

        let root = SnapshotRoot {
            hive: "HKCU".to_owned(),
            path: path.clone(),
            label: "test".to_owned(),
        };
        let data = capture(&[root], &AtomicBool::new(false)).unwrap();
        assert!(!data.entries.is_empty());
        assert!(
            data.entries
                .iter()
                .any(|entry| entry.values.iter().any(|value| value.name == "Test"))
        );

        drop(key);
        let _ = CURRENT_USER.remove_tree(&path);
    }
}
