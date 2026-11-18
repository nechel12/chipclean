//! Состояние страницы «Анализ диска».

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, TryRecvError, unbounded};

use super::disk_analysis::{self, DiskAnalysisEvent, DiskFileEntry};

pub struct DiskAnalysisState {
    pub roots: Vec<String>,
    pub selected_root: String,
    pub scanning: bool,
    pub cancelled: bool,
    pub progress_collected: usize,
    pub progress_current: String,
    pub top_files: Vec<DiskFileEntry>,
    pub total_bytes: u64,
    pub files_count: u64,
    pub elapsed: Duration,
    pub error: Option<String>,
    started: Instant,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<DiskAnalysisEvent>>,
    handle: Option<JoinHandle<()>>,
}

impl DiskAnalysisState {
    pub fn new() -> Self {
        let roots = disk_analysis::list_drives();
        let selected_root = roots.first().cloned().unwrap_or_else(|| "C:\\".to_owned());
        Self {
            roots,
            selected_root,
            scanning: false,
            cancelled: false,
            progress_collected: 0,
            progress_current: String::new(),
            top_files: Vec::new(),
            total_bytes: 0,
            files_count: 0,
            elapsed: Duration::default(),
            error: None,
            started: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
        }
    }

    pub fn start_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.top_files.clear();
        self.total_bytes = 0;
        self.files_count = 0;
        self.elapsed = Duration::default();
        self.error = None;
        self.cancelled = false;
        self.progress_collected = 0;
        self.progress_current.clear();
        self.started = Instant::now();
        self.scanning = true;

        let root = PathBuf::from(&self.selected_root);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        self.handle = Some(disk_analysis::spawn(root, cancel, tx));
    }

    pub fn cancel_scan(&mut self) {
        if self.scanning {
            self.cancelled = true;
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(DiskAnalysisEvent::Started) => {}
                Ok(DiskAnalysisEvent::Progress { collected, current }) => {
                    self.progress_collected = collected;
                    self.progress_current = current;
                }
                Ok(DiskAnalysisEvent::Finished {
                    top_files,
                    total_bytes,
                    files_count,
                    elapsed,
                    cancelled,
                }) => {
                    self.top_files = top_files;
                    self.total_bytes = total_bytes;
                    self.files_count = files_count;
                    self.elapsed = elapsed;
                    self.cancelled = cancelled;
                    finished = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.scanning = false;
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
            self.rx = None;
        }
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning
    }
}
