use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

use crossbeam_channel::{Receiver, TryRecvError, unbounded};

use crate::config::Settings;

use super::{
    catalog,
    cleaner::{self, CleanEvent},
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

#[derive(Debug, Default)]
pub enum CleanState {
    #[default]
    Idle,
    Cleaning {
        total_categories: usize,
        done_categories: usize,
        current: Option<CategoryId>,
        bytes: u64,
        files: u64,
        errors: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmStage {
    Review,
    Sensitive,
    Advanced,
}

pub struct CleanupState {
    pub categories: Vec<CleanupCategory>,
    pub selected: BTreeSet<CategoryId>,
    pub results: BTreeMap<CategoryId, ScanOutcome>,
    pub scan: ScanState,
    pub clean: CleanState,
    pub cancel_requested: bool,
    pub clean_cancel_requested: bool,
    pub pending_cleanup: Option<ConfirmStage>,
    pub sensitive_ack: bool,
    pub advanced_ack: bool,
    selection_before_max: Option<BTreeSet<CategoryId>>,
    scan_cutoff: Option<SystemTime>,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<ScanEvent>>,
    handle: Option<JoinHandle<()>>,
    clean_cancel: Arc<AtomicBool>,
    clean_rx: Option<Receiver<CleanEvent>>,
    clean_handle: Option<JoinHandle<()>>,
}

impl CleanupState {
    pub fn new(settings: &Settings) -> Self {
        let categories = catalog::catalog();
        let defaults: BTreeSet<_> = categories
            .iter()
            .filter(|category| category.default_enabled)
            .map(|category| category.id)
            .collect();

        let (selected, selection_before_max) = if settings.max_cleanup {
            let all = categories
                .iter()
                .map(|category| category.id)
                .collect::<BTreeSet<_>>();
            (all, Some(defaults))
        } else {
            (defaults, None)
        };

        Self {
            categories,
            selected,
            results: BTreeMap::new(),
            scan: ScanState::Idle,
            clean: CleanState::Idle,
            cancel_requested: false,
            clean_cancel_requested: false,
            pending_cleanup: None,
            sensitive_ack: false,
            advanced_ack: false,
            selection_before_max,
            scan_cutoff: None,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
            clean_cancel: Arc::new(AtomicBool::new(false)),
            clean_rx: None,
            clean_handle: None,
        }
    }

    pub fn set_max_cleanup(&mut self, enabled: bool) {
        if enabled {
            if self.selection_before_max.is_none() {
                self.selection_before_max = Some(self.selected.clone());
            }
            self.selected = self.categories.iter().map(|category| category.id).collect();
        } else if let Some(previous) = self.selection_before_max.take() {
            self.selected = previous;
        } else {
            self.selected = self
                .categories
                .iter()
                .filter(|category| category.default_enabled)
                .map(|category| category.id)
                .collect();
        }
    }

    pub fn start_scan(&mut self, downloads_days: u32) {
        if self.is_scanning() || self.is_cleaning() {
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
        self.scan_cutoff = Some(SystemTime::now());
        self.clean = CleanState::Idle;
        self.pending_cleanup = None;
        self.sensitive_ack = false;
        self.advanced_ack = false;
        self.cancel_requested = false;
        self.clean_cancel_requested = false;
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

    pub fn selected_has_max_only(&self) -> bool {
        self.categories
            .iter()
            .any(|category| self.selected.contains(&category.id) && category.max_only)
    }

    pub fn category(&self, id: CategoryId) -> Option<&CleanupCategory> {
        self.categories.iter().find(|category| category.id == id)
    }

    pub fn can_clean(&self) -> bool {
        if self.is_scanning() || self.is_cleaning() {
            return false;
        }
        if !matches!(
            self.scan,
            ScanState::Done {
                cancelled: false,
                ..
            }
        ) {
            return false;
        }
        if self.selected.is_empty() {
            return false;
        }

        let all_scanned = self.selected.iter().all(|id| self.results.contains_key(id));
        let any_data = self.selected.iter().any(|id| {
            self.results
                .get(id)
                .is_some_and(|outcome| outcome.bytes > 0 || outcome.files > 0)
        });

        all_scanned && any_data
    }

    pub fn cleanable_categories(&self) -> Vec<CleanupCategory> {
        self.categories
            .iter()
            .filter(|category| {
                self.selected.contains(&category.id)
                    && self
                        .results
                        .get(&category.id)
                        .is_some_and(|outcome| outcome.bytes > 0 || outcome.files > 0)
            })
            .cloned()
            .collect()
    }

    pub fn cleanable_totals(&self) -> (u64, u64, u64) {
        let mut bytes = 0_u64;
        let mut files = 0_u64;
        let mut errors = 0_u64;

        for category in &self.categories {
            if !self.selected.contains(&category.id) {
                continue;
            }
            if let Some(outcome) = self.results.get(&category.id)
                && (outcome.bytes > 0 || outcome.files > 0)
            {
                bytes = bytes.saturating_add(outcome.bytes);
                files = files.saturating_add(outcome.files);
                errors = errors.saturating_add(outcome.errors);
            }
        }

        (bytes, files, errors)
    }

    pub fn cleanable_has_extra_confirmation(&self) -> bool {
        self.cleanable_categories()
            .iter()
            .any(|category| category.extra_confirmation)
    }

    pub fn cleanable_has_max_only(&self) -> bool {
        self.cleanable_categories()
            .iter()
            .any(|category| category.max_only)
    }

    pub fn request_cleanup(&mut self) -> bool {
        if !self.can_clean() {
            return false;
        }

        self.pending_cleanup = Some(ConfirmStage::Review);
        self.sensitive_ack = false;
        self.advanced_ack = false;
        true
    }

    pub fn cancel_cleanup_confirmation(&mut self) {
        self.pending_cleanup = None;
        self.sensitive_ack = false;
        self.advanced_ack = false;
    }

    pub fn start_clean(
        &mut self,
        downloads_days: u32,
        allow_sensitive: bool,
        allow_max_only: bool,
    ) -> Result<(), String> {
        if !self.can_clean() {
            return Err("Сначала завершите сканирование выбранных разделов.".to_owned());
        }

        if self.cleanable_has_extra_confirmation() && !allow_sensitive {
            return Err(
                "Для «Загрузок», корзины и карантина требуется дополнительное подтверждение."
                    .to_owned(),
            );
        }

        if self.cleanable_has_max_only() && !allow_max_only {
            return Err(
                "Для Windows.old и загруженных обновлений требуется отдельное подтверждение."
                    .to_owned(),
            );
        }

        let categories = self.cleanable_categories();
        if categories.is_empty() {
            return Err("Нет файлов для очистки.".to_owned());
        }

        self.pending_cleanup = None;
        self.sensitive_ack = false;
        self.advanced_ack = false;
        self.clean_cancel_requested = false;
        self.clean_cancel = Arc::new(AtomicBool::new(false));
        self.clean = CleanState::Cleaning {
            total_categories: categories.len(),
            done_categories: 0,
            current: None,
            bytes: 0,
            files: 0,
            errors: 0,
            started: Instant::now(),
        };

        let (tx, rx) = unbounded();
        self.clean_rx = Some(rx);
        self.clean_handle = Some(cleaner::spawn(
            categories,
            downloads_days,
            self.scan_cutoff,
            Arc::clone(&self.clean_cancel),
            tx,
        ));

        Ok(())
    }

    pub fn cancel_clean(&mut self) {
        if self.is_cleaning() {
            self.clean_cancel_requested = true;
            self.clean_cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_cleaning(&self) -> bool {
        matches!(self.clean, CleanState::Cleaning { .. })
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
                    break;
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

    pub fn poll_clean(&mut self) {
        let Some(rx) = self.clean_rx.take() else {
            return;
        };

        let mut terminal = false;

        loop {
            match rx.try_recv() {
                Ok(CleanEvent::Started { total_categories }) => {
                    if let CleanState::Cleaning {
                        total_categories: current_total,
                        ..
                    } = &mut self.clean
                    {
                        *current_total = total_categories;
                    }
                }
                Ok(CleanEvent::CategoryStarted { id }) => {
                    if let CleanState::Cleaning { current, .. } = &mut self.clean {
                        *current = Some(id);
                    }
                }
                Ok(CleanEvent::Progress {
                    bytes,
                    files,
                    errors,
                    done_categories,
                    total_categories,
                    current,
                }) => {
                    if let CleanState::Cleaning {
                        bytes: current_bytes,
                        files: current_files,
                        errors: current_errors,
                        done_categories: current_done,
                        total_categories: current_total,
                        current: current_category,
                        ..
                    } = &mut self.clean
                    {
                        *current_bytes = bytes;
                        *current_files = files;
                        *current_errors = errors;
                        *current_done = done_categories;
                        *current_total = total_categories;
                        *current_category = current;
                    }
                }
                Ok(CleanEvent::Finished {
                    bytes,
                    files,
                    errors,
                    elapsed,
                    cancelled,
                }) => {
                    self.clean = CleanState::Done {
                        bytes,
                        files,
                        errors,
                        elapsed,
                        cancelled,
                    };
                    self.scan = ScanState::Idle;
                    self.results.clear();
                    terminal = true;
                    break;
                }
                Ok(CleanEvent::Failed(message)) => {
                    self.clean = CleanState::Failed(message);
                    self.scan = ScanState::Idle;
                    self.results.clear();
                    terminal = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.is_cleaning() {
                        self.clean =
                            CleanState::Failed("Поток очистки завершился неожиданно".to_owned());
                        self.scan = ScanState::Idle;
                        self.results.clear();
                    }
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

impl Drop for CleanupState {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_cleanup_selects_every_section_and_restores_selection() {
        let settings = Settings::default();
        let mut state = CleanupState::new(&settings);
        let defaults = state
            .categories
            .iter()
            .filter(|category| category.default_enabled)
            .count();
        let total = state.categories.len();

        assert_eq!(state.selected_count(), defaults);
        assert!(!state.selected.contains(&CategoryId::UpdateCache));
        assert!(!state.selected.contains(&CategoryId::WindowsOld));
        assert!(!state.selected.contains(&CategoryId::Downloads));
        assert!(!state.selected.contains(&CategoryId::RecycleBin));

        state.set_max_cleanup(true);
        assert_eq!(state.selected_count(), total);
        assert!(state.selected.contains(&CategoryId::UpdateCache));
        assert!(state.selected.contains(&CategoryId::WindowsOld));
        assert!(state.selected.contains(&CategoryId::Downloads));
        assert!(state.selected.contains(&CategoryId::RecycleBin));

        state.set_max_cleanup(false);
        assert_eq!(state.selected_count(), defaults);
        assert!(!state.selected.contains(&CategoryId::Downloads));
        assert!(!state.selected.contains(&CategoryId::RecycleBin));
    }

    #[test]
    fn max_cleanup_from_settings_selects_every_section() {
        let settings = Settings {
            max_cleanup: true,
            downloads_older_than_days: 7,
            background_cleanup_enabled: false,
            background_cleanup_hour: 12,
            background_cleanup_minute: 0,
            startup_check_enabled: false,
            lang: "ru".to_owned(),
        };
        let state = CleanupState::new(&settings);

        assert_eq!(state.selected_count(), state.categories.len());
    }

    #[test]
    fn max_only_sections_need_a_separate_acknowledgement() {
        let settings = Settings::default();
        let mut state = CleanupState::new(&settings);
        state.selected = [CategoryId::UpdateCache].into_iter().collect();
        state.results.insert(
            CategoryId::UpdateCache,
            ScanOutcome {
                bytes: 10,
                files: 1,
                errors: 0,
            },
        );
        state.scan = ScanState::Done {
            bytes: 10,
            files: 1,
            errors: 0,
            elapsed: Duration::ZERO,
            cancelled: false,
        };

        assert!(state.cleanable_has_max_only());

        state.advanced_ack = true;
        assert!(state.request_cleanup());
        assert!(!state.advanced_ack);

        let err = state.start_clean(7, false, false).unwrap_err();
        assert!(err.contains("Windows.old"));
    }
}
