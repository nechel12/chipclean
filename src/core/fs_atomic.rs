use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(value)?)
}

pub fn write_json_atomic_compact<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_atomic(path, &serde_json::to_vec(value)?)
}

/// Атомарно заменяет файл произвольными байтами (например, скачанной базой).
pub fn write_bytes_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    write_atomic(path, contents)
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("некорректный путь: {}", path.display()))?;

    fs::create_dir_all(parent)
        .with_context(|| format!("не удалось создать {}", parent.display()))?;

    let nonce = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        nonce
    ));

    let result = write_temp(&temp, contents);
    if let Err(err) = result {
        let _ = fs::remove_file(&temp);
        return Err(err).with_context(|| format!("не удалось записать {}", path.display()));
    }

    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(err).with_context(|| format!("не удалось заменить {}", path.display()));
    }

    Ok(())
}

fn write_temp(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn replaces_an_existing_file_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, b"{\"value\":1}").unwrap();

        write_json_atomic(&path, &json!({ "value": 2 })).unwrap();

        let parsed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(parsed["value"], 2);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("state.json")]);
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("state.json");

        write_json_atomic(&path, &json!({ "ok": true })).unwrap();

        assert!(path.is_file());
    }

    #[test]
    fn rejects_a_path_without_a_file_name() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_json_atomic(dir.path(), &json!({})).is_err());
    }
}
