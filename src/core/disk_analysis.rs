//! Сканирование диска: сбор крупнейших файлов и суммарного объёма.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct DiskFileEntry {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug)]
pub enum DiskAnalysisEvent {
    Started,
    Progress {
        collected: usize,
        current: String,
    },
    Finished {
        top_files: Vec<DiskFileEntry>,
        total_bytes: u64,
        files_count: u64,
        elapsed: Duration,
        cancelled: bool,
    },
}

pub fn spawn(
    root: PathBuf,
    cancel: Arc<AtomicBool>,
    tx: Sender<DiskAnalysisEvent>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("chipclean-disk-analysis".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let _ = tx.send(DiskAnalysisEvent::Started);

            let mut files: Vec<DiskFileEntry> = Vec::new();
            let mut total_bytes = 0u64;
            let mut files_count = 0u64;
            let mut cancelled = false;

            for entry in WalkDir::new(&root).follow_links(false) {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                if entry.file_type().is_file()
                    && let Ok(metadata) = entry.metadata()
                {
                    let size = metadata.len();
                    total_bytes = total_bytes.saturating_add(size);
                    files_count += 1;
                    files.push(DiskFileEntry {
                        path: entry.path().to_path_buf(),
                        size,
                    });
                    if files.len().is_multiple_of(20_000) {
                        let _ = tx.send(DiskAnalysisEvent::Progress {
                            collected: files.len(),
                            current: entry.path().display().to_string(),
                        });
                    }
                }
            }

            files.sort_by_key(|left| std::cmp::Reverse(left.size));
            files.truncate(300);

            let _ = tx.send(DiskAnalysisEvent::Finished {
                top_files: files,
                total_bytes,
                files_count,
                elapsed: started.elapsed(),
                cancelled,
            });
        })
        .expect("не удалось создать поток анализа диска")
}

/// Список доступных томов (буквы дисков) через GetLogicalDrives.
pub fn list_drives() -> Vec<String> {
    use windows::Win32::Storage::FileSystem::GetLogicalDrives;
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for index in 0..26 {
        if mask & (1 << index) != 0 {
            drives.push(format!("{}:\\", (b'A' + index as u8) as char));
        }
    }
    drives
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_list_has_c_drive_on_windows() {
        let drives = list_drives();
        assert!(!drives.is_empty(), "не найдено ни одного тома");
        assert!(drives.iter().any(|drive| drive.starts_with('C')));
    }
}
