use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, ErrorKind, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use serde_json::json;

use super::{
    cleaner::CleanOutcome,
    model::{CategoryId, CleanupCategory},
};

const MAX_JOURNAL_BYTES: u64 = 10 * 1024 * 1024;

// Serializes open+rotate across worker threads so two concurrent rotations
// cannot destroy the freshly rotated archive. Single-instance rules out a
// second process, so a process-wide lock is sufficient.
static JOURNAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub struct Journal {
    writer: BufWriter<File>,
    path: PathBuf,
}

impl Journal {
    pub fn open() -> Result<Self> {
        Self::open_at(crate::config::journal_path())
    }

    pub(crate) fn open_at(path: PathBuf) -> Result<Self> {
        Self::open_at_with_limit(path, MAX_JOURNAL_BYTES)
    }

    fn open_at_with_limit(path: PathBuf, max_bytes: u64) -> Result<Self> {
        let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("не удалось создать {}", parent.display()))?;
        }

        if let Err(err) = rotate_if_needed(&path, max_bytes) {
            tracing::warn!(%err, path = %path.display(), "failed to rotate operations journal");
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("не удалось открыть {}", path.display()))?;

        Ok(Self {
            writer: BufWriter::new(file),
            path,
        })
    }

    pub fn started(&mut self, categories: &[CleanupCategory], downloads_days: u32) -> Result<()> {
        let categories: Vec<_> = categories
            .iter()
            .map(|category| category.id.key())
            .collect();
        self.write(json!({
            "ts": timestamp(),
            "event": "cleanup_started",
            "categories": categories,
            "downloads_older_than_days": downloads_days,
        }))
    }

    pub fn delete(&mut self, category: CategoryId, path: &Path, bytes: u64) -> Result<()> {
        self.write(json!({
            "ts": timestamp(),
            "event": "delete",
            "category": category.key(),
            "path": path.to_string_lossy(),
            "bytes": bytes,
            "result": "ok",
        }))
    }

    pub fn failure(&mut self, category: CategoryId, path: &Path, error: &str) -> Result<()> {
        self.write(json!({
            "ts": timestamp(),
            "event": "delete",
            "category": category.key(),
            "path": path.to_string_lossy(),
            "bytes": 0,
            "result": "error",
            "error": error,
        }))
    }

    pub fn recycle_bin(&mut self, category: CategoryId, items: u64, bytes: u64) -> Result<()> {
        self.write(json!({
            "ts": timestamp(),
            "event": "empty_recycle_bin",
            "category": category.key(),
            "items": items,
            "bytes": bytes,
            "result": "ok",
        }))
    }

    pub fn finished(
        &mut self,
        outcome: &CleanOutcome,
        elapsed: Duration,
        cancelled: bool,
    ) -> Result<()> {
        self.write(json!({
            "ts": timestamp(),
            "event": "cleanup_finished",
            "bytes": outcome.bytes,
            "files": outcome.files,
            "errors": outcome.errors,
            "elapsed_ms": elapsed.as_millis(),
            "cancelled": cancelled,
        }))?;
        self.flush()
    }

    pub fn record(&mut self, value: serde_json::Value) -> Result<()> {
        self.write(value)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.writer
            .flush()
            .with_context(|| format!("не удалось записать {}", self.path.display()))
    }

    fn write(&mut self, value: serde_json::Value) -> Result<()> {
        serde_json::to_writer(&mut self.writer, &value)
            .with_context(|| format!("не удалось записать {}", self.path.display()))?;
        self.writer
            .write_all(b"\n")
            .with_context(|| format!("не удалось записать {}", self.path.display()))
    }
}

fn rotate_if_needed(path: &Path, max_bytes: u64) -> Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось проверить {}", path.display()));
        }
    };
    if metadata.len() < max_bytes {
        return Ok(());
    }

    let backup = rotated_path(path);
    match fs::remove_file(&backup) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| format!("не удалось удалить {}", backup.display()));
        }
    }
    fs::rename(path, &backup).with_context(|| {
        format!(
            "не удалось переместить {} в {}",
            path.display(),
            backup.display()
        )
    })
}

fn rotated_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".1");
    PathBuf::from(name)
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.jsonl");
        let mut journal = Journal::open_at(path.clone()).unwrap();

        journal.started(&[], 7).unwrap();
        journal
            .delete(CategoryId::TempUser, Path::new(r"C:\Temp\a.tmp"), 12)
            .unwrap();
        journal
            .finished(
                &CleanOutcome {
                    bytes: 12,
                    files: 1,
                    errors: 0,
                },
                Duration::from_millis(5),
                false,
            )
            .unwrap();

        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("\"event\":\"cleanup_started\""));
        assert!(text.contains("\"path\":\"C:\\\\Temp\\\\a.tmp\""));
        assert!(text.contains("\"event\":\"cleanup_finished\""));
    }

    #[test]
    fn rotates_an_oversized_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.jsonl");
        let backup = rotated_path(&path);
        fs::write(&path, b"old\n").unwrap();
        fs::write(&backup, b"stale\n").unwrap();

        let mut journal = Journal::open_at_with_limit(path.clone(), 4).unwrap();
        journal
            .record(json!({ "event": "after_rotation" }))
            .unwrap();
        journal.flush().unwrap();

        assert_eq!(fs::read(&backup).unwrap(), b"old\n");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("after_rotation"));
        assert!(!text.contains("stale"));
    }

    #[test]
    fn keeps_a_small_journal_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("operations.jsonl");
        fs::write(&path, b"old\n").unwrap();

        let mut journal = Journal::open_at_with_limit(path.clone(), 1024).unwrap();
        journal.record(json!({ "event": "appended" })).unwrap();
        journal.flush().unwrap();

        assert!(!rotated_path(&path).exists());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("old"));
        assert!(text.contains("appended"));
    }
}
