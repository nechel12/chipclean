use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Instant, SystemTime},
};

use crossbeam_channel::Sender;
use walkdir::WalkDir;

use super::model::{CategoryId, CleanupCategory, ScanTarget};

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOutcome {
    pub bytes: u64,
    pub files: u64,
    pub errors: u64,
}

#[derive(Debug)]
pub enum ScanEvent {
    Started {
        total: usize,
    },
    CategoryStarted {
        id: CategoryId,
    },
    CategoryFinished {
        id: CategoryId,
        outcome: ScanOutcome,
    },
    Finished {
        total_bytes: u64,
        total_files: u64,
        total_errors: u64,
        elapsed: std::time::Duration,
        cancelled: bool,
    },
}

pub(crate) struct WalkStats {
    pub errors: u64,
    pub cancelled: bool,
}

pub fn spawn(
    categories: Vec<CleanupCategory>,
    downloads_days: u32,
    cancel: Arc<AtomicBool>,
    tx: Sender<ScanEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("chipclean-scan".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let total = categories.len();
            let _ = tx.send(ScanEvent::Started { total });

            let mut total_bytes = 0_u64;
            let mut total_files = 0_u64;
            let mut total_errors = 0_u64;
            let mut cancelled = false;

            for category in &categories {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }

                if tx
                    .send(ScanEvent::CategoryStarted { id: category.id })
                    .is_err()
                {
                    return;
                }

                let outcome = scan_category(category, downloads_days, &cancel);
                total_bytes += outcome.bytes;
                total_files += outcome.files;
                total_errors += outcome.errors;

                if tx
                    .send(ScanEvent::CategoryFinished {
                        id: category.id,
                        outcome,
                    })
                    .is_err()
                {
                    return;
                }

                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
            }

            let _ = tx.send(ScanEvent::Finished {
                total_bytes,
                total_files,
                total_errors,
                elapsed: started.elapsed(),
                cancelled,
            });
        })
        .expect("не удалось создать поток сканирования")
}

fn scan_category(
    category: &CleanupCategory,
    downloads_days: u32,
    cancel: &AtomicBool,
) -> ScanOutcome {
    let mut outcome = ScanOutcome::default();

    for target in &category.targets {
        if let ScanTarget::RecycleBin = target {
            match crate::win::recycle_bin::query() {
                Ok(info) => {
                    outcome.bytes += info.bytes;
                    outcome.files += info.items;
                }
                Err(err) => {
                    tracing::debug!(%err, "recycle bin query failed");
                    outcome.errors += 1;
                }
            }
        }

        if let ScanTarget::ChipCleanQuarantine = target {
            let stats = crate::core::quarantine::stats();
            outcome.bytes = outcome.bytes.saturating_add(stats.bytes);
            outcome.files = outcome.files.saturating_add(stats.files);
        }
    }

    let stats = walk_category_files(category, downloads_days, cancel, |_, metadata| {
        outcome.bytes = outcome.bytes.saturating_add(metadata.len());
        outcome.files = outcome.files.saturating_add(1);
    });
    outcome.errors = outcome.errors.saturating_add(stats.errors);

    outcome
}

pub(crate) fn walk_category_files(
    category: &CleanupCategory,
    downloads_days: u32,
    cancel: &AtomicBool,
    mut visit: impl FnMut(&Path, &std::fs::Metadata),
) -> WalkStats {
    let mut stats = WalkStats {
        errors: 0,
        cancelled: false,
    };

    'targets: for target in &category.targets {
        let ScanTarget::Path {
            source,
            min_age_days,
            max_depth,
        } = target
        else {
            continue;
        };

        if cancel.load(Ordering::Relaxed) {
            stats.cancelled = true;
            break;
        }

        let days = if category.id == CategoryId::Downloads {
            downloads_days
        } else {
            *min_age_days
        };

        for root in source.resolve() {
            let mut walker = WalkDir::new(&root)
                .follow_links(false)
                .follow_root_links(false);
            if *max_depth > 0 {
                walker = walker.max_depth(*max_depth);
            }

            for (index, entry) in walker.into_iter().filter_entry(should_visit).enumerate() {
                if index % 256 == 0 && cancel.load(Ordering::Relaxed) {
                    stats.cancelled = true;
                    break 'targets;
                }

                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        tracing::trace!(path = %root.display(), %err, "walk error");
                        stats.errors += 1;
                        continue;
                    }
                };

                if !entry.file_type().is_file() || entry.file_type().is_symlink() {
                    continue;
                }

                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => {
                        stats.errors += 1;
                        continue;
                    }
                };

                if days > 0 && !is_older_than(metadata.modified().ok(), days) {
                    continue;
                }

                visit(entry.path(), &metadata);
            }

            if cancel.load(Ordering::Relaxed) {
                stats.cancelled = true;
                break 'targets;
            }
        }
    }

    stats
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

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

        if let Ok(metadata) = std::fs::symlink_metadata(entry.path())
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return false;
        }
    }

    true
}

fn is_older_than(modified: Option<SystemTime>, days: u32) -> bool {
    let Some(modified) = modified else {
        return false;
    };

    match SystemTime::now().duration_since(modified) {
        Ok(age) => age.as_secs() >= u64::from(days) * 86_400,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn age_filter_keeps_recent_files() {
        let now = SystemTime::now();
        assert!(!is_older_than(Some(now), 1));
        assert!(!is_older_than(None, 7));
    }

    #[test]
    fn age_filter_accepts_old_files() {
        let old = SystemTime::now() - Duration::from_secs(8 * 86_400);
        assert!(is_older_than(Some(old), 7));
        assert!(!is_older_than(Some(old), 30));
    }
}
