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
    pending_ids: Vec<String>,
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
            pending_ids: Vec::new(),
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
        self.cancel_clean();
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
        self.cancel_clean();
        self.scans.remove(id);
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }

    pub fn is_selected(&self, target: &DataTarget) -> bool {
        self.selected.contains(&target.id)
    }

    pub fn select_defaults(&mut self) {
        self.cancel_clean();
        self.scans.clear();
        for target in &self.targets {
            if target.default_enabled {
                self.selected.insert(target.id.clone());
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.cancel_clean();
        self.scans.clear();
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

    pub fn pending_targets(&self) -> Vec<DataTarget> {
        self.targets
            .iter()
            .filter(|target| self.pending_ids.contains(&target.id))
            .cloned()
            .collect()
    }

    pub fn pending_stats(&self) -> (u64, u64, u64) {
        let mut files = 0;
        let mut bytes = 0;
        let mut registry = 0;
        for target in self
            .targets
            .iter()
            .filter(|target| self.pending_ids.contains(&target.id))
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
        let selected = self.selected_targets();
        if selected.is_empty() {
            self.set_message("Выберите секции для очистки.", true);
            return;
        }
        let unscanned = selected
            .iter()
            .filter(|target| !self.scans.contains_key(&target.id))
            .count();
        if unscanned > 0 {
            self.set_message(
                format!("Сначала проверьте выбранные секции: без проверки {unscanned}."),
                true,
            );
            return;
        }
        let (files, bytes, registry) = self.selected_stats();
        if files == 0 && bytes == 0 && registry == 0 {
            self.set_message("Нет данных для очистки: сначала выполните проверку.", true);
            return;
        }
        self.pending_ids = selected.into_iter().map(|target| target.id).collect();
        self.pending_clean = true;
        self.confirm_ack = false;
    }

    pub fn cancel_clean(&mut self) {
        self.pending_clean = false;
        self.confirm_ack = false;
        self.pending_ids.clear();
    }

    pub fn confirm_clean(&mut self) {
        if !self.confirm_ack {
            self.set_message("Подтвердите необратимость удаления.", true);
            return;
        }
        let targets = self.pending_targets();
        if targets.is_empty() {
            self.cancel_clean();
            self.set_message(
                "Набор для очистки не зафиксирован; повторите проверку.",
                true,
            );
            return;
        }
        if targets
            .iter()
            .any(|target| !self.scans.contains_key(&target.id))
        {
            self.cancel_clean();
            self.set_message("Проверка устарела; выполните её повторно.", true);
            return;
        }
        self.pending_clean = false;
        self.confirm_ack = false;
        self.pending_ids.clear();

        let mut report = CleanReport::default();
        for target in &targets {
            let target_report = privacy::clean_target(target);
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
        for target in &targets {
            let scan = privacy::scan_target(target);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::privacy::FileSpec;
    use std::fs;

    fn target(id: &str, path: &std::path::Path) -> DataTarget {
        DataTarget {
            id: id.to_owned(),
            title: id.to_owned(),
            summary: String::new(),
            group: "Test".to_owned(),
            warning: None,
            default_enabled: true,
            files: vec![FileSpec {
                pattern: path.display().to_string(),
                recurse: false,
            }],
            history: Vec::new(),
            registry: Vec::new(),
            exclude_files: Vec::new(),
            exclude_registry: Vec::new(),
        }
    }

    fn state_with(targets: Vec<DataTarget>) -> CommunityState {
        CommunityState {
            db_path: String::new(),
            targets,
            selected: HashSet::new(),
            scans: HashMap::new(),
            filter: String::new(),
            pending_clean: false,
            confirm_ack: false,
            last_report: None,
            message: None,
            pending_ids: Vec::new(),
            loaded: true,
        }
    }

    #[test]
    fn requires_a_scan_before_cleaning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.tmp");
        fs::write(&path, b"data").unwrap();
        let mut state = state_with(vec![target("a", &path)]);
        state.selected.insert("a".to_owned());

        state.request_clean();
        assert!(!state.pending_clean);
        assert!(
            state
                .message
                .as_ref()
                .unwrap()
                .text
                .contains("Сначала проверьте")
        );

        state.scan();
        state.request_clean();
        assert!(state.pending_clean);
    }

    #[test]
    fn toggling_selection_invalidates_its_scan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.tmp");
        fs::write(&path, b"data").unwrap();
        let mut state = state_with(vec![target("a", &path)]);
        state.selected.insert("a".to_owned());
        state.scan();
        assert!(state.scans.contains_key("a"));

        state.toggle("a");
        assert!(state.scans.is_empty());
        assert!(!state.selected.contains("a"));

        state.toggle("a");
        assert!(state.scans.is_empty());
        state.request_clean();
        assert!(!state.pending_clean);
    }

    #[test]
    fn cleanup_uses_the_frozen_selection() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.tmp");
        let second = dir.path().join("b.tmp");
        fs::write(&first, b"a").unwrap();
        fs::write(&second, b"b").unwrap();
        let mut state = state_with(vec![target("a", &first), target("b", &second)]);
        state.selected.insert("a".to_owned());
        state.scan();
        state.request_clean();
        assert!(state.pending_clean);
        assert_eq!(state.pending_targets().len(), 1);

        state.selected.insert("b".to_owned());
        state.confirm_ack = true;
        state.confirm_clean();

        assert!(!first.exists());
        assert!(second.exists());
        assert!(!state.message.as_ref().unwrap().is_error);
    }
}
