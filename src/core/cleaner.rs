use std::{
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;

use super::{
    journal::Journal,
    model::{CategoryId, CleanupCategory, ScanTarget},
    scanner::walk_category_files,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct CleanOutcome {
    pub bytes: u64,
    pub files: u64,
    pub errors: u64,
}

#[derive(Debug)]
pub enum CleanEvent {
    Started {
        total_categories: usize,
    },
    CategoryStarted {
        id: CategoryId,
    },
    Progress {
        bytes: u64,
        files: u64,
        errors: u64,
        done_categories: usize,
        total_categories: usize,
        current: Option<CategoryId>,
    },
    Finished {
        bytes: u64,
        files: u64,
        errors: u64,
        elapsed: Duration,
        cancelled: bool,
    },
    Failed(String),
}

pub fn spawn(
    categories: Vec<CleanupCategory>,
    downloads_days: u32,
    cancel: Arc<AtomicBool>,
    tx: Sender<CleanEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("chipclean-clean".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let total_categories = categories.len();

            let mut journal = match Journal::open() {
                Ok(journal) => journal,
                Err(err) => {
                    let _ = tx.send(CleanEvent::Failed(format!(
                        "Не удалось открыть журнал операций: {err}"
                    )));
                    return;
                }
            };

            if let Err(err) = journal.started(&categories, downloads_days) {
                let _ = tx.send(CleanEvent::Failed(format!(
                    "Не удалось записать начало очистки: {err}"
                )));
                return;
            }

            let _ = tx.send(CleanEvent::Started { total_categories });

            let mut outcome = CleanOutcome::default();
            let mut cancelled = false;
            let mut fatal: Option<String> = None;
            let mut last_emit = Instant::now();

            for (index, category) in categories.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }

                if tx
                    .send(CleanEvent::CategoryStarted { id: category.id })
                    .is_err()
                {
                    cancel.store(true, Ordering::Relaxed);
                    return;
                }

                let category_outcome = clean_category(
                    category,
                    downloads_days,
                    &cancel,
                    &mut journal,
                    &tx,
                    total_categories,
                    index,
                    outcome,
                    &mut last_emit,
                    &mut fatal,
                );

                outcome.bytes = outcome.bytes.saturating_add(category_outcome.bytes);
                outcome.files = outcome.files.saturating_add(category_outcome.files);
                outcome.errors = outcome.errors.saturating_add(category_outcome.errors);

                if let Err(err) = journal.flush() {
                    fatal = Some(format!("Журнал операций недоступен: {err}"));
                    cancel.store(true, Ordering::Relaxed);
                }

                let _ = tx.send(CleanEvent::Progress {
                    bytes: outcome.bytes,
                    files: outcome.files,
                    errors: outcome.errors,
                    done_categories: index + 1,
                    total_categories,
                    current: Some(category.id),
                });

                if fatal.is_some() {
                    break;
                }

                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
            }

            let elapsed = started.elapsed();

            if let Some(message) = fatal {
                let _ = journal.finished(&outcome, elapsed, true);
                let _ = tx.send(CleanEvent::Failed(message));
                return;
            }

            if let Err(err) = journal.finished(&outcome, elapsed, cancelled) {
                let _ = tx.send(CleanEvent::Failed(format!(
                    "Не удалось завершить журнал операций: {err}"
                )));
                return;
            }

            let _ = tx.send(CleanEvent::Finished {
                bytes: outcome.bytes,
                files: outcome.files,
                errors: outcome.errors,
                elapsed,
                cancelled,
            });
        })
        .expect("не удалось создать поток очистки")
}

#[allow(clippy::too_many_arguments)]
fn clean_category(
    category: &CleanupCategory,
    downloads_days: u32,
    cancel: &AtomicBool,
    journal: &mut Journal,
    tx: &Sender<CleanEvent>,
    total_categories: usize,
    done_categories: usize,
    base: CleanOutcome,
    last_emit: &mut Instant,
    fatal: &mut Option<String>,
) -> CleanOutcome {
    let mut outcome = CleanOutcome::default();

    for target in &category.targets {
        if !matches!(target, ScanTarget::RecycleBin) {
            continue;
        }

        if cancel.load(Ordering::Relaxed) {
            return outcome;
        }

        let before = crate::win::recycle_bin::query().unwrap_or_default();
        match crate::win::recycle_bin::empty() {
            Ok(()) => {
                outcome.bytes = outcome.bytes.saturating_add(before.bytes);
                outcome.files = outcome.files.saturating_add(before.items);

                if let Err(err) = journal.recycle_bin(category.id, before.items, before.bytes) {
                    *fatal = Some(format!("Журнал операций недоступен: {err}"));
                    cancel.store(true, Ordering::Relaxed);
                    return outcome;
                }
            }
            Err(err) => {
                outcome.errors = outcome.errors.saturating_add(1);
                if let Err(journal_err) =
                    journal.failure(category.id, Path::new("<recycle-bin>"), &err.to_string())
                {
                    *fatal = Some(format!("Журнал операций недоступен: {journal_err}"));
                    cancel.store(true, Ordering::Relaxed);
                    return outcome;
                }
            }
        }
    }

    let stats = walk_category_files(category, downloads_days, cancel, |path, metadata| {
        if fatal.is_some() {
            return;
        }

        match safe_delete_file(path) {
            Ok(()) => {
                outcome.bytes = outcome.bytes.saturating_add(metadata.len());
                outcome.files = outcome.files.saturating_add(1);

                if let Err(err) = journal.delete(category.id, path, metadata.len()) {
                    *fatal = Some(format!("Журнал операций недоступен: {err}"));
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            Err(err) => {
                outcome.errors = outcome.errors.saturating_add(1);

                if let Err(journal_err) = journal.failure(category.id, path, &err.to_string()) {
                    *fatal = Some(format!("Журнал операций недоступен: {journal_err}"));
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }

        if last_emit.elapsed() >= Duration::from_millis(120) {
            let _ = tx.send(CleanEvent::Progress {
                bytes: base.bytes.saturating_add(outcome.bytes),
                files: base.files.saturating_add(outcome.files),
                errors: base.errors.saturating_add(outcome.errors),
                done_categories,
                total_categories,
                current: Some(category.id),
            });
            *last_emit = Instant::now();
        }
    });

    outcome.errors = outcome.errors.saturating_add(stats.errors);
    outcome
}

fn safe_delete_file(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "не обычный файл",
        ));
    }

    std::fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Group, PathSource, Risk};
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    use windows::core::PCWSTR;

    #[test]
    fn deletes_regular_files_and_refuses_directories() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache.tmp");
        std::fs::write(&file, b"temporary").unwrap();

        assert!(safe_delete_file(&file).is_ok());
        assert!(!file.exists());

        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        assert!(safe_delete_file(&subdir).is_err());
        assert!(subdir.exists());
    }

    #[test]
    fn cleans_a_temporary_tree_and_writes_journal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("a.tmp"), b"1234").unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("nested").join("b.tmp"), b"123456").unwrap();

        let category = CleanupCategory {
            id: CategoryId::TempUser,
            title: "test",
            summary: "test",
            icon: "",
            group: Group::System,
            risk: Risk::Safe,
            requires_admin: false,
            extra_confirmation: false,
            default_enabled: true,
            max_only: false,
            targets: vec![ScanTarget::path(PathSource::Literal(root.clone()))],
        };

        let cancel = AtomicBool::new(false);
        let (tx, _rx) = crossbeam_channel::unbounded();
        let journal_dir = tempfile::tempdir().unwrap();
        let journal_path = journal_dir.path().join("journal.jsonl");
        let mut journal = Journal::open_at(journal_path.clone()).unwrap();
        let mut last_emit = Instant::now();
        let mut fatal = None;

        let outcome = clean_category(
            &category,
            7,
            &cancel,
            &mut journal,
            &tx,
            1,
            0,
            CleanOutcome::default(),
            &mut last_emit,
            &mut fatal,
        );
        journal.flush().unwrap();

        assert!(fatal.is_none());
        assert_eq!(outcome.files, 2);
        assert_eq!(outcome.bytes, 10);
        assert!(!root.join("a.tmp").exists());
        assert!(!root.join("nested").join("b.tmp").exists());
        assert!(root.join("nested").exists());

        let journal = std::fs::read_to_string(journal_path).unwrap();
        assert_eq!(journal.lines().count(), 2);
    }

    #[test]
    #[ignore = "destructive: empties the entire Recycle Bin; requires CHIPCLEAN_TEST_RECYCLE_BIN=1"]
    fn cleans_recycle_bin_end_to_end() {
        if std::env::var_os("CHIPCLEAN_TEST_RECYCLE_BIN").is_none() {
            eprintln!("CHIPCLEAN_TEST_RECYCLE_BIN is not set; skipping destructive test");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chipclean-recycle-bin-test.txt");
        std::fs::write(&file, b"recycle bin deletion test").unwrap();
        assert!(
            send_to_recycle_bin(&file),
            "failed to send test file to the Recycle Bin"
        );
        assert!(!file.exists());

        let before = crate::win::recycle_bin::query().unwrap();
        assert!(
            before.items >= 1,
            "test file did not appear in the Recycle Bin"
        );

        let category = crate::core::catalog::catalog()
            .into_iter()
            .find(|category| category.id == CategoryId::RecycleBin)
            .unwrap();

        let cancel = AtomicBool::new(false);
        let (tx, _rx) = crossbeam_channel::unbounded();
        let journal_dir = tempfile::tempdir().unwrap();
        let journal_path = journal_dir.path().join("journal.jsonl");
        let mut journal = Journal::open_at(journal_path.clone()).unwrap();
        let mut last_emit = Instant::now();
        let mut fatal = None;

        let outcome = clean_category(
            &category,
            7,
            &cancel,
            &mut journal,
            &tx,
            1,
            0,
            CleanOutcome::default(),
            &mut last_emit,
            &mut fatal,
        );
        journal.flush().unwrap();

        assert!(fatal.is_none());
        assert!(outcome.files >= 1);
        assert!(outcome.bytes > 0);

        let after = crate::win::recycle_bin::query().unwrap_or_default();
        assert_eq!(after.items, 0, "Recycle Bin should be empty");

        let journal = std::fs::read_to_string(journal_path).unwrap();
        assert!(journal.contains("\"event\":\"empty_recycle_bin\""));
    }

    fn send_to_recycle_bin(path: &Path) -> bool {
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.extend([0, 0]);

        let mut operation = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0)
                as u16,
            ..Default::default()
        };

        let result = unsafe { SHFileOperationW(&mut operation) };
        result == 0 && !operation.fAnyOperationsAborted.as_bool()
    }
}
