use std::collections::{HashMap, HashSet};

use crate::{
    config,
    core::{
        community,
        privacy::{self, CleanReport, DataTarget, TargetScan},
    },
};

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

pub struct CommunityState {
    pub db_path: String,
    pub targets: Vec<DataTarget>,
    pub selected: HashSet<String>,
    pub scans: HashMap<String, TargetScan>,
    pub filter: String,
    pub pending_clean: bool,
    pub confirm_ack: bool,
    pub last_report: Option<CleanReport>,
    pub message: Option<Message>,
    loaded: bool,
}

impl CommunityState {
    pub fn new() -> Self {
        Self {
            db_path: config::community_db_path().display().to_string(),
            targets: Vec::new(),
            selected: HashSet::new(),
            scans: HashMap::new(),
            filter: String::new(),
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
        if std::path::Path::new(&self.db_path).is_file() {
            self.load();
        } else {
            self.set_message(
                "База winapp2.ini не найдена. Укажите путь или положите файл в папку базы.",
                true,
            );
        }
    }

    pub fn load(&mut self) {
        match community::parse_database(std::path::Path::new(&self.db_path)) {
            Ok(targets) => {
                self.targets = targets;
                self.selected.clear();
                self.scans.clear();
                self.last_report = None;
                self.set_message(format!("Загружено секций: {}", self.targets.len()), false);
            }
            Err(err) => {
                self.targets.clear();
                self.selected.clear();
                self.scans.clear();
                self.set_message(err.to_string(), true);
            }
        }
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }

    pub fn is_selected(&self, target: &DataTarget) -> bool {
        self.selected.contains(&target.id)
    }

    pub fn select_defaults(&mut self) {
        for target in &self.targets {
            if target.default_enabled {
                self.selected.insert(target.id.clone());
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub fn selected_targets(&self) -> Vec<DataTarget> {
        self.targets
            .iter()
            .filter(|target| self.selected.contains(&target.id))
            .cloned()
            .collect()
    }

    pub fn selected_stats(&self) -> (u64, u64, u64) {
        let mut files = 0;
        let mut bytes = 0;
        let mut registry = 0;
        for target in self
            .targets
            .iter()
            .filter(|target| self.selected.contains(&target.id))
        {
            if let Some(scan) = self.scans.get(&target.id) {
                files += scan.files;
                bytes += scan.bytes;
                registry += scan.registry;
            }
        }
        (files, bytes, registry)
    }

    pub fn scan(&mut self) {
        let targets = self.selected_targets();
        if targets.is_empty() {
            self.set_message("Выберите секции для проверки.", true);
            return;
        }
        self.scans.clear();
        let mut errors = 0_u64;
        for target in &targets {
            let scan = privacy::scan_target(target);
            errors += scan.errors;
            self.scans.insert(target.id.clone(), scan);
        }
        self.set_message(
            if errors > 0 {
                format!("Проверено: {} · ошибок: {errors}", targets.len())
            } else {
                format!("Проверено секций: {}", targets.len())
            },
            errors > 0,
        );
    }

    pub fn request_clean(&mut self) {
        let (files, bytes, registry) = self.selected_stats();
        if files == 0 && bytes == 0 && registry == 0 {
            self.set_message("Нет данных для очистки: сначала выполните проверку.", true);
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
            self.set_message("Подтвердите необратимость удаления.", true);
            return;
        }
        self.pending_clean = false;
        self.confirm_ack = false;
        let mut report = CleanReport::default();
        for target in self.selected_targets() {
            let target_report = privacy::clean_target(&target);
            report.files += target_report.files;
            report.bytes += target_report.bytes;
            report.registry += target_report.registry;
            report.errors.extend(target_report.errors);
        }
        self.set_message(
            if report.errors.is_empty() {
                format!(
                    "Удалено файлов: {} · записей реестра: {}",
                    report.files, report.registry
                )
            } else {
                format!(
                    "Удалено файлов: {} · ошибок: {}",
                    report.files,
                    report.errors.len()
                )
            },
            !report.errors.is_empty(),
        );
        self.last_report = Some(report);
        for target in self.selected_targets() {
            let scan = privacy::scan_target(&target);
            self.scans.insert(target.id.clone(), scan);
        }
    }

    fn set_message(&mut self, text: impl Into<String>, is_error: bool) {
        self.message = Some(Message {
            text: text.into(),
            is_error,
        });
    }
}
