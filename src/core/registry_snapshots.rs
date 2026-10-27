use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    config,
    core::registry_editor::{EditorHive, full_path, join_path, list_values, open_key},
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
        format!("{}\\{}", self.hive, self.path)
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

pub fn create_snapshot(name: &str, roots: &[SnapshotRoot]) -> Result<SnapshotMeta> {
    if roots.is_empty() {
        bail!("Выберите хотя бы один раздел");
    }
    let name = name.trim();
    let name = if name.is_empty() {
        format!("Снимок {}", Utc::now().format("%d.%m.%Y %H:%M"))
    } else {
        name.to_owned()
    };

    let data = capture(roots)?;
    let id = format!("snapshot-{}", Utc::now().format("%Y%m%d-%H%M%S-%3f"));
    let file = format!("{id}.json");
    let dir = config::snapshots_dir();
    fs::create_dir_all(&dir).with_context(|| format!("не удалось создать {}", dir.display()))?;
    let path = dir.join(&file);
    let raw = serde_json::to_vec(&data)?;
    fs::write(&path, raw).with_context(|| format!("не удалось записать {}", path.display()))?;

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
    let path = config::snapshots_dir().join(&meta.file);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("не удалось удалить {}", path.display()))?;
    }
    let mut index = load_index(&index_path())?;
    index.entries.retain(|entry| entry.id != meta.id);
    save_index(&index)
}

pub fn compare(meta: &SnapshotMeta) -> Result<SnapshotDiff> {
    let path = config::snapshots_dir().join(&meta.file);
    let raw =
        fs::read(&path).with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let before: SnapshotData = serde_json::from_slice(&raw)
        .with_context(|| format!("не удалось разобрать {}", path.display()))?;
    let after_data = capture(&meta.roots)?;
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

    diff.added_keys.sort();
    diff.removed_keys.sort();
    Ok(diff)
}

fn capture(roots: &[SnapshotRoot]) -> Result<SnapshotData> {
    let mut entries = Vec::new();
    let mut total = 0_usize;
    let mut truncated = false;

    for root in roots {
        let hive = parse_hive(&root.hive)?;
        let root_path = root.path.trim_matches('\\').to_owned();
        let mut queue = VecDeque::from([root_path.clone()]);

        while let Some(path) = queue.pop_front() {
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
    match value.to_ascii_uppercase().as_str() {
        "HKCU" => Ok(EditorHive::Hkcu),
        "HKLM" => Ok(EditorHive::Hklm),
        other => bail!("неизвестная ветка снимка: {other}"),
    }
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
    let path = index_path();
    fs::write(&path, serde_json::to_vec_pretty(index)?)
        .with_context(|| format!("не удалось записать {}", path.display()))
}

pub fn snapshot_size(meta: &SnapshotMeta) -> Result<u64> {
    let path = config::snapshots_dir().join(&meta.file);
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
    fn captures_a_small_root() {
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
        let data = capture(&[root]).unwrap();
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
