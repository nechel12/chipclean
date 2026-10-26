use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, TryRecvError, unbounded};

use crate::config::Settings;

use super::{
    catalog,
    model::{CategoryId, CleanupCategory},
    scanner::{self, ScanEvent, ScanOutcome},
};

#[derive(Debug, Default)]
pub enum ScanState {
    #[default]
    Idle,
    Scanning {
        total: usize,
        done: usize,
        current: Option<CategoryId>,
        started: Instant,
    },
    Done {
        bytes: u64,
        files: u64,
        errors: u64,
        elapsed: Duration,
        cancelled: bool,
    },
    Failed(String),
}

pub struct CleanupState {
    pub categories: Vec<CleanupCategory>,
    pub selected: BTreeSet<CategoryId>,
    pub results: BTreeMap<CategoryId, ScanOutcome>,
    pub scan: ScanState,
    pub cancel_requested: bool,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<ScanEvent>>,
    handle: Option<JoinHandle<()>>,
}

impl CleanupState {
    pub fn new(settings: &Settings) -> Self {
        let categories = catalog::catalog();
        let mut selected = BTreeSet::new();

        for category in &categories {
            if category.default_enabled || (settings.max_cleanup && category.max_only) {
                selected.insert(category.id);
            }
        }

        Self {
            categories,
            selected,
            results: BTreeMap::new(),
            scan: ScanState::Idle,
            cancel_requested: false,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
        }
    }

    pub fn set_max_cleanup(&mut self, enabled: bool) {
        for category in &self.categories {
            if !category.max_only {
                continue;
            }
            if enabled {
                self.selected.insert(category.id);
            } else {
                self.selected.remove(&category.id);
            }
        }
    }

    pub fn start_scan(&mut self, downloads_days: u32) {
        if self.is_scanning() {
            return;
        }

        let categories: Vec<_> = self
            .categories
            .iter()
            .filter(|category| self.selected.contains(&category.id))
            .cloned()
            .collect();

        if categories.is_empty() {
            return;
        }

        self.results.clear();
        self.cancel_requested = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.scan = ScanState::Scanning {
            total: categories.len(),
            done: 0,
            current: None,
            started: Instant::now(),
        };

        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        self.handle = Some(scanner::spawn(
            categories,
            downloads_days,
            Arc::clone(&self.cancel),
            tx,
        ));
    }

    pub fn cancel_scan(&mut self) {
        if self.is_scanning() {
            self.cancel_requested = true;
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_scanning(&self) -> bool {
        matches!(self.scan, ScanState::Scanning { .. })
    }

    pub fn progress(&self) -> (usize, usize, Option<CategoryId>) {
        match &self.scan {
            ScanState::Scanning {
                total,
                done,
                current,
                ..
            } => (*done, *total, *current),
            _ => (0, 0, None),
        }
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    pub fn selected_bytes(&self) -> u64 {
        self.selected
            .iter()
            .filter_map(|id| self.results.get(id))
            .map(|outcome| outcome.bytes)
            .sum()
    }

    pub fn selected_has_extra_confirmation(&self) -> bool {
        self.categories
            .iter()
            .any(|category| self.selected.contains(&category.id) && category.extra_confirmation)
    }

    pub fn category(&self, id: CategoryId) -> Option<&CleanupCategory> {
        self.categories.iter().find(|category| category.id == id)
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };

        let mut terminal = false;

        loop {
            match rx.try_recv() {
                Ok(ScanEvent::Started { total }) => {
                    if let ScanState::Scanning {
                        total: current_total,
                        ..
                    } = &mut self.scan
                    {
                        *current_total = total;
                    }
                }
                Ok(ScanEvent::CategoryStarted { id }) => {
                    if let ScanState::Scanning { current, .. } = &mut self.scan {
                        *current = Some(id);
                    }
                }
                Ok(ScanEvent::CategoryFinished { id, outcome }) => {
                    self.results.insert(id, outcome);
                    if let ScanState::Scanning { done, .. } = &mut self.scan {
                        *done += 1;
                    }
                }
                Ok(ScanEvent::Finished {
                    total_bytes,
                    total_files,
                    total_errors,
                    elapsed,
                    cancelled,
                }) => {
                    self.scan = ScanState::Done {
                        bytes: total_bytes,
                        files: total_files,
                        errors: total_errors,
                        elapsed,
                        cancelled,
                    };
                    terminal = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.scan =
                        ScanState::Failed("Поток сканирования завершился неожиданно".to_owned());
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
}

impl Drop for CleanupState {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_cleanup_toggles_only_advanced_sections() {
        let settings = Settings::default();
        let mut state = CleanupState::new(&settings);
        let initial = state.selected_count();

        assert!(!state.selected.contains(&CategoryId::UpdateCache));
        assert!(!state.selected.contains(&CategoryId::WindowsOld));

        state.set_max_cleanup(true);
        assert!(state.selected_count() > initial);
        assert!(state.selected.contains(&CategoryId::UpdateCache));
        assert!(state.selected.contains(&CategoryId::WindowsOld));

        state.set_max_cleanup(false);
        assert!(!state.selected.contains(&CategoryId::UpdateCache));
        assert!(!state.selected.contains(&CategoryId::WindowsOld));
    }
}
