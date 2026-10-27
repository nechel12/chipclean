use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

use crate::{
    config,
    core::{
        community,
        job::{self, Job},
        privacy::{self, CleanReport, DataTarget, TargetScan},
    },
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

pub struct CommunityState {
    pub db_path: String,
    pub targets: Vec<DataTarget>,
    pub selected: HashSet<String>,
    pub scans: HashMap<String, TargetScan>,
    pub filter: String,
    pub pending_clean: bool,
    pub confirm_ack: bool,
    pub risk_ack: bool,
    pub last_report: Option<CleanReport>,
    pub message: Option<Message>,
    pending_ids: Vec<String>,
    loaded: bool,
    scan_job: Option<Job<ScanEvent>>,
    scan_progress: Option<Progress>,
    clean_job: Option<Job<CleanEvent>>,
    clean_progress: Option<Progress>,
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
            risk_ack: false,
            last_report: None,
            message: None,
            pending_ids: Vec::new(),
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
        if self.is_scanning() || self.is_cleaning() {
            self.set_message("Дождитесь завершения текущей операции.", true);
            return;
        }
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
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
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
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
        self.cancel_clean();
        self.scans.clear();
        for target in &self.targets {
            if target.default_enabled {
                self.selected.insert(target.id.clone());
            }
        }
    }

    pub fn clear_selection(&mut self) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
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

    pub fn pending_risky(&self) -> (u64, Vec<String>) {
        let mut count = 0;
        let mut items = Vec::new();
        for target in self
            .targets
            .iter()
            .filter(|target| self.pending_ids.contains(&target.id))
        {
            if let Some(scan) = self.scans.get(&target.id) {
                count += scan.risky_count;
                for item in &scan.risky_items {
                    if items.len() < 30 {
                        items.push(item.clone());
                    }
                }
            }
        }
        (count, items)
    }

    pub fn pending_blocked(&self) -> u64 {
        self.targets
            .iter()
            .filter(|target| self.pending_ids.contains(&target.id))
            .filter_map(|target| self.scans.get(&target.id))
            .map(|scan| scan.blocked_count)
            .sum()
    }

    pub fn scan(&mut self) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
        let targets = self.selected_targets();
        if targets.is_empty() {
            self.set_message("Выберите секции для проверки.", true);
            return;
        }
        let total = targets.len();
        self.scans.clear();
        self.message = None;
        self.scan_progress = Some(Progress {
            total,
            done: 0,
            current: None,
            rescan: false,
        });
        self.scan_job = Some(Job::spawn("chipclean-community-scan", move |cancel, tx| {
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
            self.set_message(
                "Проверка прервана: поток сканирования завершился неожиданно.",
                true,
            );
            return;
        }

        let errors: u64 = self.scans.values().map(|scan| scan.errors).sum();
        let done = self.scans.len();
        let total = progress.map(|progress| progress.total).unwrap_or(done);
        let text = if cancelled {
            format!("Проверка отменена: {done} из {total}.")
        } else if errors > 0 {
            format!("Проверено: {done} · ошибок: {errors}")
        } else {
            format!("Проверено секций: {done}")
        };
        self.set_message(text, errors > 0);
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

    pub fn request_clean(&mut self) {
        if self.is_scanning() || self.is_cleaning() {
            return;
        }
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
        self.risk_ack = false;
    }

    pub fn cancel_clean(&mut self) {
        self.pending_clean = false;
        self.confirm_ack = false;
        self.risk_ack = false;
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
        let (risky, _) = self.pending_risky();
        if risky > 0 && !self.risk_ack {
            self.set_message("Подтвердите очистку путей вне безопасных каталогов.", true);
            return;
        }
        self.pending_clean = false;
        self.confirm_ack = false;
        self.risk_ack = false;
        self.pending_ids.clear();

        let total = targets.len();
        self.clean_progress = Some(Progress {
            total,
            done: 0,
            current: None,
            rescan: false,
        });
        self.clean_job = Some(Job::spawn(
            "chipclean-community-clean",
            move |cancel, tx| {
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
            },
        ));
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
                    self.clean_finished(report, cancelled);
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
            self.set_message("Очистка прервана: поток завершился неожиданно.", true);
        }
    }

    fn clean_finished(&mut self, report: CleanReport, cancelled: bool) {
        let text = if report.errors.is_empty() {
            format!(
                "{}Удалено файлов: {} · записей реестра: {}",
                if cancelled {
                    "Очистка отменена. "
                } else {
                    ""
                },
                report.files,
                report.registry
            )
        } else {
            format!(
                "Удалено файлов: {} · ошибок: {}",
                report.files,
                report.errors.len()
            )
        };
        let is_error = !report.errors.is_empty();
        self.last_report = Some(report);
        self.set_message(text, is_error);
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
    use std::{fs, thread, time::Duration, time::Instant};

    fn wait_scan(state: &mut CommunityState) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.is_scanning() && Instant::now() < deadline {
            state.poll_scan();
            if state.is_scanning() {
                thread::sleep(Duration::from_millis(2));
            }
        }
        assert!(!state.is_scanning(), "scan did not finish in time");
    }

    fn wait_clean(state: &mut CommunityState) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.is_cleaning() && Instant::now() < deadline {
            state.poll_clean();
            if state.is_cleaning() {
                thread::sleep(Duration::from_millis(2));
            }
        }
        assert!(!state.is_cleaning(), "clean did not finish in time");
    }

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
            risk_ack: false,
            last_report: None,
            message: None,
            pending_ids: Vec::new(),
            loaded: true,
            scan_job: None,
            scan_progress: None,
            clean_job: None,
            clean_progress: None,
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
        wait_scan(&mut state);
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
        wait_scan(&mut state);
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
        wait_scan(&mut state);
        state.request_clean();
        assert!(state.pending_clean);
        assert_eq!(state.pending_targets().len(), 1);

        state.selected.insert("b".to_owned());
        state.confirm_ack = true;
        state.confirm_clean();
        wait_clean(&mut state);

        assert!(!first.exists());
        assert!(second.exists());
        assert!(!state.message.as_ref().unwrap().is_error);
    }
}
