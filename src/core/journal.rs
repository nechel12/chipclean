use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
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

pub struct Journal {
    writer: BufWriter<File>,
    path: PathBuf,
}

impl Journal {
    pub fn open() -> Result<Self> {
        Self::open_at(crate::config::journal_path())
    }

    pub(crate) fn open_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("не удалось создать {}", parent.display()))?;
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
}
