use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, TryRecvError, unbounded};

use super::registry::{
    self, RegistryCategory, RegistryCategoryId, RegistryCleanEvent, RegistryFinding,
    RegistryScanEvent,
};

#[derive(Debug, Default)]
pub enum RegistryScanState {
    #[default]
    Idle,
    Scanning {
        total: usize,
        done: usize,
        current: Option<RegistryCategoryId>,
        started: Instant,
    },
    Done {
        findings: usize,
        elapsed: Duration,
        cancelled: bool,
    },
    Failed(String),
}

#[derive(Debug, Default)]
pub enum RegistryCleanState {
    #[default]
    Idle,
    Cleaning {
        total: usize,
        done: usize,
        deleted: usize,
        errors: usize,
        current: String,
        backup_dir: Option<PathBuf>,
        started: Instant,
    },
    Done {
        deleted: usize,
        errors: usize,
        elapsed: Duration,
        cancelled: bool,
        backup_dir: PathBuf,
    },
    Failed(String),
}

pub struct RegistryState {
    pub categories: Vec<RegistryCategory>,
    pub selected: BTreeSet<RegistryCategoryId>,
    pub scanned_categories: BTreeSet<RegistryCategoryId>,
    pub findings: Vec<RegistryFinding>,
    pub scan: RegistryScanState,
    pub clean: RegistryCleanState,
    pub pending_confirmation: bool,
    pub confirm_ack: bool,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<RegistryScanEvent>>,
    handle: Option<JoinHandle<()>>,
    clean_cancel: Arc<AtomicBool>,
    clean_rx: Option<Receiver<RegistryCleanEvent>>,
    clean_handle: Option<JoinHandle<()>>,
}

impl RegistryState {
    pub fn new() -> Self {
        let categories = registry::catalog();
        let selected = categories
            .iter()
            .filter(|category| category.default_enabled)
            .map(|category| category.id)
            .collect();

        Self {
            categories,
            selected,
            scanned_categories: BTreeSet::new(),
            findings: Vec::new(),
            scan: RegistryScanState::Idle,
            clean: RegistryCleanState::Idle,
            pending_confirmation: false,
            confirm_ack: false,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
            clean_cancel: Arc::new(AtomicBool::new(false)),
            clean_rx: None,
            clean_handle: None,
        }
    }

    pub fn start_scan(&mut self) {
        if self.is_scanning() || self.is_cleaning() || self.selected.is_empty() {
            return;
        }

        self.findings.clear();
        self.scanned_categories = self.selected.clone();
        self.pending_confirmation = false;
        self.confirm_ack = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.scan = RegistryScanState::Scanning {
            total: self.selected.len(),
            done: 0,
            current: None,
            started: Instant::now(),
        };

        let categories: Vec<_> = self.selected.iter().copied().collect();
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        self.handle = Some(registry::spawn_scan(
            categories,
            Arc::clone(&self.cancel),
            tx,
        ));
    }

    pub fn cancel_scan(&mut self) {
        if self.is_scanning() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_scanning(&self) -> bool {
        matches!(self.scan, RegistryScanState::Scanning { .. })
    }

    pub fn scan_progress(&self) -> (usize, usize, Option<RegistryCategoryId>) {
        match &self.scan {
            RegistryScanState::Scanning {
                total,
                done,
                current,
                ..
            } => (*done, *total, *current),
            _ => (0, 0, None),
        }
    }

    pub fn can_clean(&self) -> bool {
        !self.is_scanning()
            && !self.is_cleaning()
            && !self.findings.is_empty()
            && self.selected == self.scanned_categories
            && matches!(
                self.scan,
                RegistryScanState::Done {
                    cancelled: false,
                    ..
                }
            )
    }

    pub fn is_cleaning(&self) -> bool {
        matches!(self.clean, RegistryCleanState::Cleaning { .. })
    }

    pub fn request_cleanup(&mut self) -> bool {
        if !self.can_clean() {
            return false;
        }

        self.pending_confirmation = true;
        self.confirm_ack = false;
        true
    }

    pub fn cancel_confirmation(&mut self) {
        self.pending_confirmation = false;
        self.confirm_ack = false;
    }

    pub fn start_clean(&mut self) -> Result<(), String> {
        if !self.can_clean() {
            return Err("Сначала завершите проверку выбранных разделов.".to_owned());
        }
        if !self.confirm_ack {
            return Err("Подтвердите изменение реестра.".to_owned());
        }

        self.pending_confirmation = false;
        self.confirm_ack = false;
        self.clean_cancel = Arc::new(AtomicBool::new(false));
        self.clean = RegistryCleanState::Cleaning {
            total: self.findings.len(),
            done: 0,
            deleted: 0,
            errors: 0,
            current: String::new(),
            backup_dir: None,
            started: Instant::now(),
        };

        let findings = self.findings.clone();
        let (tx, rx) = unbounded();
        self.clean_rx = Some(rx);
        self.clean_handle = Some(registry::spawn_clean(
            findings,
            Arc::clone(&self.clean_cancel),
            tx,
        ));

        Ok(())
    }

    pub fn cancel_clean(&mut self) {
        if self.is_cleaning() {
            self.clean_cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn poll_scan(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };

        let mut terminal = false;

        loop {
            match rx.try_recv() {
                Ok(RegistryScanEvent::Started { total }) => {
                    if let RegistryScanState::Scanning {
                        total: current_total,
                        ..
                    } = &mut self.scan
                    {
                        *current_total = total;
                    }
                }
                Ok(RegistryScanEvent::CategoryStarted { id }) => {
                    if let RegistryScanState::Scanning { current, .. } = &mut self.scan {
                        *current = Some(id);
                    }
                }
                Ok(RegistryScanEvent::CategoryFinished { findings }) => {
                    self.findings.extend(findings);
                    if let RegistryScanState::Scanning { done, .. } = &mut self.scan {
                        *done += 1;
                    }
                }
                Ok(RegistryScanEvent::Finished {
                    total_findings,
                    elapsed,
                    cancelled,
                }) => {
                    self.scan = RegistryScanState::Done {
                        findings: total_findings,
                        elapsed,
                        cancelled,
                    };
                    terminal = true;
                    break;
                }
                Ok(RegistryScanEvent::Failed(message)) => {
                    self.scan = RegistryScanState::Failed(message);
                    terminal = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.scan =
                        RegistryScanState::Failed("Поток проверки реестра завершился".to_owned());
                    terminal = true;
                    break;
                }
            }
        }

        if terminal {
            self.rx = None;
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        } else {
            self.rx = Some(rx);
        }
    }

    pub fn poll_clean(&mut self) {
        let Some(rx) = self.clean_rx.take() else {
            return;
        };

        let mut terminal = false;

        loop {
            match rx.try_recv() {
                Ok(RegistryCleanEvent::Started { total }) => {
                    if let RegistryCleanState::Cleaning {
                        total: current_total,
                        ..
                    } = &mut self.clean
                    {
                        *current_total = total;
                    }
                }
                Ok(RegistryCleanEvent::BackupCreated { dir }) => {
                    if let RegistryCleanState::Cleaning { backup_dir, .. } = &mut self.clean {
                        *backup_dir = Some(dir);
                    }
                }
                Ok(RegistryCleanEvent::Progress {
                    done,
                    total,
                    deleted,
                    errors,
                    current,
                    backup_dir,
                }) => {
                    if let RegistryCleanState::Cleaning {
                        done: current_done,
                        total: current_total,
                        deleted: current_deleted,
                        errors: current_errors,
                        current: current_text,
                        backup_dir: current_backup,
                        ..
                    } = &mut self.clean
                    {
                        *current_done = done;
                        *current_total = total;
                        *current_deleted = deleted;
                        *current_errors = errors;
                        *current_text = current;
                        *current_backup = Some(backup_dir);
                    }
                }
                Ok(RegistryCleanEvent::Finished {
                    deleted,
                    errors,
                    elapsed,
                    cancelled,
                    backup_dir,
                }) => {
                    self.clean = RegistryCleanState::Done {
                        deleted,
                        errors,
                        elapsed,
                        cancelled,
                        backup_dir,
                    };
                    self.findings.clear();
                    self.scanned_categories.clear();
                    self.scan = RegistryScanState::Idle;
                    terminal = true;
                    break;
                }
                Ok(RegistryCleanEvent::Failed(message)) => {
                    self.clean = RegistryCleanState::Failed(message);
                    self.findings.clear();
                    self.scanned_categories.clear();
                    self.scan = RegistryScanState::Idle;
                    terminal = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.clean =
                        RegistryCleanState::Failed("Поток очистки реестра завершился".to_owned());
                    self.findings.clear();
                    self.scanned_categories.clear();
                    self.scan = RegistryScanState::Idle;
                    terminal = true;
                    break;
                }
            }
        }

        if terminal {
            self.clean_rx = None;
            if let Some(handle) = self.clean_handle.take() {
                let _ = handle.join();
            }
        } else {
            self.clean_rx = Some(rx);
        }
    }
}

impl Drop for RegistryState {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.clean_cancel.store(true, Ordering::Relaxed);

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.clean_handle.take() {
            let _ = handle.join();
        }
    }
}
