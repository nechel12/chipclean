use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

use crate::core::{
    job::{self, Job},
    privacy::{self, CleanReport, DataTarget, TargetScan},
};

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

#[derive(Clone)]
pub struct Progress {
    pub total: usize,
    pub done: usize,
    pub current: Option<String>,
    pub rescan: bool,
}

enum ScanEvent {
    Started { total: usize },
    TargetStarted { id: String },
    Scanned { id: String, scan: TargetScan },
    Finished { cancelled: bool },
}

enum CleanEvent {
    TargetStarted {
        id: String,
    },
    RescanStarted {
        total: usize,
    },
    RescanTargetStarted {
        id: String,
    },
    Scanned {
        id: String,
        scan: TargetScan,
    },
    Finished {
        report: CleanReport,
        cancelled: bool,
    },
}

pub struct PrivacyState {
    pub targets: Vec<DataTarget>,
    pub selected: HashSet<String>,
    pub scans: HashMap<String, TargetScan>,
    pub pending_clean: bool,
    pub confirm_ack: bool,
    pub last_report: Option<CleanReport>,
    pub message: Option<Message>,
    loaded: bool,
    scan_job: Option<Job<ScanEvent>>,
    scan_progress: Option<Progress>,
    clean_job: Option<Job<CleanEvent>>,
    clean_progress: Option<Progress>,
}

impl PrivacyState {
    pub fn new() -> Self {
        let targets = privacy::personal_targets();
        let selected = targets
            .iter()
            .filter(|target| target.default_enabled)
            .map(|target| target.id.clone())
            .collect();
        Self {
            targets,
            selected,
            scans: HashMap::new(),
            pending_clean: false,
            confirm_ack: false,
            last_report: None,
            message: None,
            loaded: false,
            scan_job: None,
            scan_progress: None,
            clean_job: None,
            clean_progress: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.scan();
    }

    pub fn scan(&mut self) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
        let targets = self.targets.clone();
        let total = targets.len();
        self.scans.clear();
        self.last_report = None;
        self.message = None;
        self.scan_progress = Some(Progress {
            total,
            done: 0,
            current: None,
            rescan: false,
        });
        self.scan_job = Some(Job::spawn("chipclean-privacy-scan", move |cancel, tx| {
            let _ = tx.send(ScanEvent::Started { total });
            for target in &targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let _ = tx.send(ScanEvent::TargetStarted {
                    id: target.id.clone(),
                });
                let scan = privacy::scan_target(target);
                if tx
                    .send(ScanEvent::Scanned {
                        id: target.id.clone(),
                        scan,
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = tx.send(ScanEvent::Finished {
                cancelled: cancel.load(Ordering::Relaxed),
            });
        }));
    }

    pub fn poll_scan(&mut self) {
        let (events, disconnected) = job::drain(&self.scan_job);
        let mut finished = false;
        let mut cancelled = false;
        for event in events {
            match event {
                ScanEvent::Started { total } => {
                    if let Some(progress) = &mut self.scan_progress {
                        progress.total = total;
                    }
                }
                ScanEvent::TargetStarted { id } => {
                    if let Some(progress) = &mut self.scan_progress {
                        progress.current = Some(id);
                    }
                }
                ScanEvent::Scanned { id, scan } => {
                    self.scans.insert(id, scan);
                    if let Some(progress) = &mut self.scan_progress {
                        progress.done += 1;
                    }
                }
                ScanEvent::Finished {
                    cancelled: was_cancelled,
                } => {
                    cancelled = was_cancelled;
                    finished = true;
                }
            }
        }

        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.scan_job.take() {
            job.finish();
        }
        let progress = self.scan_progress.take();
        if !finished {
            self.message = Some(Message {
                text: "Проверка прервана: поток сканирования завершился неожиданно.".to_owned(),
                is_error: true,
            });
            return;
        }

        let errors: u64 = self.scans.values().map(|scan| scan.errors).sum();
        let done = self.scans.len();
        let total = progress.map(|progress| progress.total).unwrap_or(done);
        let text = if cancelled {
            format!("Проверка отменена: {done} из {total}.")
        } else if errors > 0 {
            format!("Проверка завершена; ошибок доступа: {errors}")
        } else {
            "Проверка завершена.".to_owned()
        };
        self.message = Some(Message {
            text,
            is_error: errors > 0,
        });
    }

    pub fn is_scanning(&self) -> bool {
        self.scan_job.is_some()
    }

    pub fn is_cleaning(&self) -> bool {
        self.clean_job.is_some()
    }

    pub fn scan_progress(&self) -> Option<Progress> {
        self.scan_progress.clone()
    }

    pub fn clean_progress(&self) -> Option<Progress> {
        self.clean_progress.clone()
    }

    pub fn cancel_scan(&mut self) {
        if let Some(job) = &self.scan_job {
            job.cancel();
        }
    }

    pub fn cancel_cleaning(&mut self) {
        if let Some(job) = &self.clean_job {
            job.cancel();
        }
    }

    pub fn toggle(&mut self, id: &str) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }

    pub fn is_selected(&self, target: &DataTarget) -> bool {
        self.selected.contains(&target.id)
    }

    pub fn selected_targets(&self) -> Vec<DataTarget> {
        self.targets
            .iter()
            .filter(|target| self.selected.contains(&target.id))
            .cloned()
            .collect()
    }

    pub fn selected_stats(&self) -> (u64, u64, u64, u64) {
        let mut files = 0;
        let mut bytes = 0;
        let mut history = 0;
        let mut registry = 0;
        for target in self
            .targets
            .iter()
            .filter(|target| self.selected.contains(&target.id))
        {
            if let Some(scan) = self.scans.get(&target.id) {
                files += scan.files;
                bytes += scan.bytes;
                history += scan.history;
                registry += scan.registry;
            }
        }
        (files, bytes, history, registry)
    }

    pub fn request_clean(&mut self) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
        let unscanned = self
            .selected_targets()
            .iter()
            .filter(|target| !self.scans.contains_key(&target.id))
            .count();
        if unscanned > 0 {
            self.message = Some(Message {
                text: format!("Сначала проверьте выбранные разделы: без проверки {unscanned}."),
                is_error: true,
            });
            return;
        }
        let (files, bytes, history, registry) = self.selected_stats();
        if files == 0 && bytes == 0 && history == 0 && registry == 0 {
            self.message = Some(Message {
                text: "Выбранные разделы пусты.".to_owned(),
                is_error: false,
            });
            return;
        }
        self.pending_clean = true;
        self.confirm_ack = false;
    }

    pub fn cancel_clean(&mut self) {
        self.pending_clean = false;
        self.confirm_ack = false;
    }

    pub fn confirm_clean(&mut self) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
        if !self.confirm_ack {
            self.message = Some(Message {
                text: "Подтвердите, что понимаете необратимость удаления.".to_owned(),
                is_error: true,
            });
            return;
        }
        self.pending_clean = false;
        self.confirm_ack = false;

        let targets = self.selected_targets();
        let total = targets.len();
        self.clean_progress = Some(Progress {
            total,
            done: 0,
            current: None,
            rescan: false,
        });
        self.clean_job = Some(Job::spawn("chipclean-privacy-clean", move |cancel, tx| {
            let mut report = CleanReport::default();
            let mut cleaned = Vec::new();
            for target in &targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let _ = tx.send(CleanEvent::TargetStarted {
                    id: target.id.clone(),
                });
                report.merge(privacy::clean_target(target));
                cleaned.push(target.clone());
            }
            if !cleaned.is_empty() {
                let _ = tx.send(CleanEvent::RescanStarted {
                    total: cleaned.len(),
                });
                for target in &cleaned {
                    let _ = tx.send(CleanEvent::RescanTargetStarted {
                        id: target.id.clone(),
                    });
                    let scan = privacy::scan_target(target);
                    if tx
                        .send(CleanEvent::Scanned {
                            id: target.id.clone(),
                            scan,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
            let _ = tx.send(CleanEvent::Finished {
                report,
                cancelled: cancel.load(Ordering::Relaxed),
            });
        }));
    }

    pub fn poll_clean(&mut self) {
        let (events, disconnected) = job::drain(&self.clean_job);
        let mut finished = false;
        for event in events {
            match event {
                CleanEvent::TargetStarted { id } => {
                    if let Some(progress) = &mut self.clean_progress {
                        progress.current = Some(id);
                    }
                }
                CleanEvent::RescanStarted { total } => {
                    if let Some(progress) = &mut self.clean_progress {
                        progress.rescan = true;
                        progress.total = total;
                        progress.done = 0;
                        progress.current = None;
                    }
                }
                CleanEvent::RescanTargetStarted { id } => {
                    if let Some(progress) = &mut self.clean_progress {
                        progress.current = Some(id);
                    }
                }
                CleanEvent::Scanned { id, scan } => {
                    self.scans.insert(id, scan);
                    if let Some(progress) = &mut self.clean_progress {
                        progress.done += 1;
                    }
                }
                CleanEvent::Finished { report, cancelled } => {
                    self.last_report = Some(report);
                    self.clean_finished(cancelled);
                    finished = true;
                }
            }
        }

        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.clean_job.take() {
            job.finish();
        }
        self.clean_progress = None;
        if !finished {
            self.message = Some(Message {
                text: "Очистка прервана: поток завершился неожиданно.".to_owned(),
                is_error: true,
            });
        }
    }

    fn clean_finished(&mut self, cancelled: bool) {
        if let Some(progress) = &mut self.clean_progress {
            progress.done = progress.total;
        }
        let report = match &self.last_report {
            Some(report) => report,
            None => return,
        };
        let text = if report.errors.is_empty() {
            format!(
                "{}Удалено файлов: {} · записей реестра: {} · посещений: {}",
                if cancelled {
                    "Очистка отменена. "
                } else {
                    ""
                },
                report.files,
                report.registry,
                report.history
            )
        } else {
            format!(
                "Удалено файлов: {} · посещений: {}; ошибок: {}",
                report.files,
                report.history,
                report.errors.len()
            )
        };
        self.message = Some(Message {
            text,
            is_error: !report.errors.is_empty(),
        });
    }
}
