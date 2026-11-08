use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::json;
use walkdir::WalkDir;

use crate::{
    config,
    core::{journal::Journal, startup, traces},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct QuarantineStats {
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Default)]
pub struct PurgeOutcome {
    pub bytes: u64,
    pub files: u64,
    pub errors: u64,
}

pub fn stats() -> QuarantineStats {
    stats_in(&quarantine_dirs())
}

pub fn purge() -> Result<PurgeOutcome> {
    let dirs = quarantine_dirs();
    let mut outcome = purge_dirs(&dirs);

    if outcome.errors == 0 {
        if let Err(err) = startup::purge_folder_quarantine_metadata() {
            tracing::warn!(%err, "failed to purge startup quarantine metadata");
            outcome.errors += 1;
        }
        if let Err(err) = traces::clear_quarantine_metadata() {
            tracing::warn!(%err, "failed to purge trace quarantine metadata");
            outcome.errors += 1;
        }
    } else {
        tracing::warn!(
            "quarantine purge had deletion errors; metadata preserved so survivors stay restorable"
        );
    }

    journal(json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "event": "chipclean_quarantine_purged",
        "bytes": outcome.bytes,
        "files": outcome.files,
        "errors": outcome.errors,
    }));

    Ok(outcome)
}

fn quarantine_dirs() -> Vec<PathBuf> {
    vec![
        config::startup_quarantine_dir(),
        config::traces_quarantine_dir(),
    ]
}

fn stats_in(dirs: &[PathBuf]) -> QuarantineStats {
    let mut stats = QuarantineStats::default();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(should_visit)
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                stats.bytes = stats.bytes.saturating_add(metadata.len());
                stats.files = stats.files.saturating_add(1);
            }
        }
    }

    stats
}

fn purge_dirs(dirs: &[PathBuf]) -> PurgeOutcome {
    let mut outcome = PurgeOutcome::default();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        let Ok(read_dir) = fs::read_dir(dir) else {
            outcome.errors += 1;
            continue;
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            let stats = if path.is_dir() {
                stats_in(std::slice::from_ref(&path))
            } else {
                fs::metadata(&path)
                    .map(|metadata| QuarantineStats {
                        bytes: metadata.len(),
                        files: 1,
                    })
                    .unwrap_or_default()
            };
            match delete_tree(&path) {
                Ok(()) => {
                    outcome.bytes = outcome.bytes.saturating_add(stats.bytes);
                    outcome.files = outcome.files.saturating_add(stats.files);
                }
                Err(err) => {
                    tracing::warn!(%err, path = %path.display(), "failed to delete quarantine item");
                    outcome.errors += 1;
                }
            }
        }
    }

    outcome
}

fn delete_tree(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;

    if metadata.is_dir() && !reparse {
        for entry in fs::read_dir(path)
            .with_context(|| format!("не удалось прочитать {}", path.display()))?
            .flatten()
        {
            delete_tree(&entry.path())?;
        }
        fs::remove_dir(path).with_context(|| format!("не удалось удалить {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("не удалось удалить {}", path.display()))?;
    }

    Ok(())
}

fn should_visit(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    if entry.file_type().is_symlink() {
        return false;
    }
    if !entry.file_type().is_dir() {
        return true;
    }

    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    if let Ok(metadata) = std::fs::symlink_metadata(entry.path())
        && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return false;
    }

    true
}

fn journal(event: serde_json::Value) {
    match Journal::open() {
        Ok(mut journal) => {
            if let Err(err) = journal.record(event) {
                tracing::warn!(%err, "failed to write quarantine journal event");
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
    fn counts_and_purges_quarantine_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let quarantine = dir.path().join("quarantine");
        fs::create_dir_all(&quarantine).unwrap();
        fs::write(quarantine.join("a.tmp"), b"1234").unwrap();
        let nested = quarantine.join("folder");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("b.tmp"), b"123456").unwrap();

        let stats = stats_in(std::slice::from_ref(&quarantine));
        assert_eq!(stats.files, 2);
        assert_eq!(stats.bytes, 10);

        let outcome = purge_dirs(std::slice::from_ref(&quarantine));
        assert_eq!(outcome.errors, 0);
        assert_eq!(outcome.files, 2);
        assert_eq!(outcome.bytes, 10);
        assert!(fs::read_dir(&quarantine).unwrap().next().is_none());
    }
}
