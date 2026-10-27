use std::collections::{HashMap, HashSet};

use crate::core::privacy::{self, CleanReport, DataTarget, TargetScan};

pub struct Message {
    pub text: String,
    pub is_error: bool,
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
        self.scans.clear();
        self.last_report = None;
        let mut errors = 0_u64;
        for target in &self.targets {
            let scan = privacy::scan_target(target);
            errors += scan.errors;
            self.scans.insert(target.id.clone(), scan);
        }
        self.message = Some(Message {
            text: if errors > 0 {
                format!("Проверка завершена; ошибок доступа: {errors}")
            } else {
                "Проверка завершена.".to_owned()
            },
            is_error: errors > 0,
        });
    }

    pub fn toggle(&mut self, id: &str) {
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
        if !self.confirm_ack {
            self.message = Some(Message {
                text: "Подтвердите, что понимаете необратимость удаления.".to_owned(),
                is_error: true,
            });
            return;
        }
        self.pending_clean = false;
        self.confirm_ack = false;

        let mut report = CleanReport::default();
        for target in self.selected_targets() {
            let target_report = privacy::clean_target(&target);
            report.files += target_report.files;
            report.bytes += target_report.bytes;
            report.history += target_report.history;
            report.registry += target_report.registry;
            report.errors.extend(target_report.errors);
        }
        self.message = Some(Message {
            text: if report.errors.is_empty() {
                format!(
                    "Удалено файлов: {} · записей реестра: {} · посещений: {}",
                    report.files, report.registry, report.history
                )
            } else {
                format!(
                    "Удалено файлов: {} · посещений: {}; ошибок: {}",
                    report.files,
                    report.history,
                    report.errors.len()
                )
            },
            is_error: !report.errors.is_empty(),
        });
        self.last_report = Some(report);
        for target in &self.targets {
            let scan = privacy::scan_target(target);
            self.scans.insert(target.id.clone(), scan);
        }
    }
}
