use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, TryRecvError, unbounded};

use super::traces::{
    self, QuarantinedTrace, TraceCategory, TraceCategoryId, TraceEntry, TraceScanEvent,
};

#[derive(Debug, Default)]
pub enum TraceScanState {
    #[default]
    Idle,
    Scanning {
        total: usize,
        done: usize,
        current: Option<TraceCategoryId>,
        started: Instant,
    },
    Done {
        findings: usize,
        elapsed: Duration,
        cancelled: bool,
    },
    Failed(String),
}

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceAction {
    Quarantine,
    Delete,
}

pub struct TracesState {
    pub categories: Vec<TraceCategory>,
    pub selected: BTreeSet<TraceCategoryId>,
    pub scanned_categories: BTreeSet<TraceCategoryId>,
    pub entries: Vec<TraceEntry>,
    pub quarantined: Vec<QuarantinedTrace>,
    pub scan: TraceScanState,
    pub pending_action: Option<TraceAction>,
    pub confirm_ack: bool,
    pub message: Option<Message>,
    loaded: bool,
    cancel: Arc<AtomicBool>,
    rx: Option<Receiver<TraceScanEvent>>,
    handle: Option<JoinHandle<()>>,
}

impl TracesState {
    pub fn new() -> Self {
        let categories = traces::catalog();
        let selected = categories
            .iter()
            .filter(|category| category.default_enabled)
            .map(|category| category.id)
            .collect();

        Self {
            categories,
            selected,
            scanned_categories: BTreeSet::new(),
            entries: Vec::new(),
            quarantined: Vec::new(),
            scan: TraceScanState::Idle,
            pending_action: None,
            confirm_ack: false,
            message: None,
            loaded: false,
            cancel: Arc::new(AtomicBool::new(false)),
            rx: None,
            handle: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.loaded = true;
            self.start_scan();
        }
    }

    pub fn start_scan(&mut self) {
        if self.is_scanning() || self.selected.is_empty() {
            return;
        }

        self.entries.clear();
        self.scanned_categories = self.selected.clone();
        self.pending_action = None;
        self.confirm_ack = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.scan = TraceScanState::Scanning {
            total: self.selected.len(),
            done: 0,
            current: None,
            started: Instant::now(),
        };

        match traces::load_quarantine() {
            Ok(store) => self.quarantined = store.entries,
            Err(err) => {
                self.message = Some(Message {
                    text: format!("Не удалось загрузить карантин: {err}"),
                    is_error: true,
                });
            }
        }

        let categories: Vec<_> = self.selected.iter().copied().collect();
        let (tx, rx) = unbounded();
        self.rx = Some(rx);
        self.handle = Some(traces::spawn_scan(categories, Arc::clone(&self.cancel), tx));
    }

    pub fn cancel_scan(&mut self) {
        if self.is_scanning() {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_scanning(&self) -> bool {
        matches!(self.scan, TraceScanState::Scanning { .. })
    }

    pub fn scan_progress(&self) -> (usize, usize, Option<TraceCategoryId>) {
        match &self.scan {
            TraceScanState::Scanning {
                total,
                done,
                current,
                ..
            } => (*done, *total, *current),
            _ => (0, 0, None),
        }
    }

    pub fn can_process(&self) -> bool {
        !self.is_scanning()
            && !self.entries.is_empty()
            && self.selected == self.scanned_categories
            && self
                .entries
                .iter()
                .any(|entry| self.selected.contains(&entry.category))
    }

    pub fn selected_entries(&self) -> Vec<TraceEntry> {
        self.entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.category))
            .cloned()
            .collect()
    }

    pub fn needs_ack(&self, action: TraceAction) -> bool {
        match action {
            TraceAction::Delete => true,
            TraceAction::Quarantine => self.selected_entries().iter().any(|entry| {
                matches!(
                    entry.category,
                    TraceCategoryId::OrphanedFolders | TraceCategoryId::DeepLeftovers
                )
            }),
        }
    }

    pub fn request_action(&mut self, action: TraceAction) -> bool {
        if !self.can_process() {
            return false;
        }

        self.pending_action = Some(action);
        self.confirm_ack = false;
        true
    }

    pub fn cancel_action(&mut self) {
        self.pending_action = None;
        self.confirm_ack = false;
    }

    pub fn confirm_action(&mut self) {
        let Some(action) = self.pending_action else {
            return;
        };

        let entries = self.selected_entries();
        if entries.is_empty() {
            self.cancel_action();
            return;
        }
        if self.needs_ack(action) && !self.confirm_ack {
            self.message = Some(Message {
                text: "Подтвердите выбранное действие.".to_owned(),
                is_error: true,
            });
            return;
        }

        self.pending_action = None;
        self.confirm_ack = false;

        match action {
            TraceAction::Quarantine => match traces::quarantine_entries(&entries) {
                Ok(count) => {
                    self.message = Some(Message {
                        text: format!("Перемещено в карантин: {count}"),
                        is_error: false,
                    });
                    self.start_scan();
                }
                Err(err) => {
                    self.message = Some(Message {
                        text: format!("Не удалось переместить в карантин: {err}"),
                        is_error: true,
                    });
                }
            },
            TraceAction::Delete => {
                let outcome = traces::delete_entries(&entries);
                let mut text = format!("Удалено безвозвратно: {}", outcome.deleted);
                if !outcome.errors.is_empty() {
                    text.push_str(&format!(" · ошибок: {}", outcome.errors.len()));
                }
                self.message = Some(Message {
                    text,
                    is_error: !outcome.errors.is_empty(),
                });
                self.start_scan();
            }
        }
    }

    pub fn restore(&mut self, index: usize) {
        let Some(entry) = self.quarantined.get(index).cloned() else {
            return;
        };

        match traces::restore_quarantined(&entry) {
            Ok(()) => {
                self.message = Some(Message {
                    text: "Объект возвращён на место.".to_owned(),
                    is_error: false,
                });
                self.start_scan();
            }
            Err(err) => {
                self.message = Some(Message {
                    text: format!("Не удалось вернуть объект: {err}"),
                    is_error: true,
                });
            }
        }
    }

    pub fn poll(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };

        let mut terminal = false;

        loop {
            match rx.try_recv() {
                Ok(TraceScanEvent::Started { total }) => {
                    if let TraceScanState::Scanning {
                        total: current_total,
                        ..
                    } = &mut self.scan
                    {
                        *current_total = total;
                    }
                }
                Ok(TraceScanEvent::CategoryStarted { id }) => {
                    if let TraceScanState::Scanning { current, .. } = &mut self.scan {
                        *current = Some(id);
                    }
                }
                Ok(TraceScanEvent::CategoryFinished { entries, error, .. }) => {
                    self.entries.extend(entries);
                    if let TraceScanState::Scanning { done, .. } = &mut self.scan {
                        *done += 1;
                    }
                    if let Some(error) = error {
                        self.message = Some(Message {
                            text: error,
                            is_error: true,
                        });
                    }
                }
                Ok(TraceScanEvent::Finished {
                    total,
                    elapsed,
                    cancelled,
                }) => {
                    self.scan = TraceScanState::Done {
                        findings: total,
                        elapsed,
                        cancelled,
                    };
                    terminal = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.scan = TraceScanState::Failed("Поток поиска следов завершился".to_owned());
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

impl Drop for TracesState {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
