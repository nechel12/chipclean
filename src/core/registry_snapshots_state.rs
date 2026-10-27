use std::collections::HashSet;

use crate::core::{
    job::{self, Job},
    reg_file,
    registry_editor::{self, EditorHive},
    registry_snapshots::{self, SnapshotDiff, SnapshotMeta, SnapshotRoot},
};

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Create,
    Compare,
}

enum Event {
    Created(Result<SnapshotMeta, String>),
    Compared(Result<SnapshotDiff, String>),
}

pub struct SnapshotState {
    pub roots: Vec<SnapshotRoot>,
    pub custom_roots: Vec<SnapshotRoot>,
    pub custom_hive: EditorHive,
    pub custom_path: String,
    pub selected_roots: HashSet<String>,
    pub name: String,
    pub snapshots: Vec<SnapshotMeta>,
    pub selected: Option<String>,
    pub diff: Option<SnapshotDiff>,
    pub pending_delete: Option<SnapshotMeta>,
    pub message: Option<Message>,
    loaded: bool,
    job: Option<Job<Event>>,
    busy: Option<Busy>,
}

impl SnapshotState {
    pub fn new() -> Self {
        let roots = registry_snapshots::root_options();
        let selected_roots = roots
            .first()
            .map(|root| HashSet::from([root.display()]))
            .unwrap_or_default();
        Self {
            roots,
            custom_roots: Vec::new(),
            custom_hive: EditorHive::Hkcu,
            custom_path: String::new(),
            selected_roots,
            name: String::new(),
            snapshots: Vec::new(),
            selected: None,
            diff: None,
            pending_delete: None,
            message: None,
            loaded: false,
            job: None,
            busy: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.reload();
    }

    pub fn reload(&mut self) {
        match registry_snapshots::list_snapshots() {
            Ok(snapshots) => {
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|selected| !snapshots.iter().any(|meta| &meta.id == selected))
                {
                    self.selected = None;
                }
                self.snapshots = snapshots;
            }
            Err(err) => self.set_error(err.to_string()),
        }
    }

    pub fn toggle_root(&mut self, root: &SnapshotRoot) {
        let key = root.display();
        if !self.selected_roots.remove(&key) {
            self.selected_roots.insert(key);
        }
    }

    pub fn is_root_selected(&self, root: &SnapshotRoot) -> bool {
        self.selected_roots.contains(&root.display())
    }

    pub fn add_custom_root(&mut self) {
        if self.is_busy() {
            return;
        }
        let typed = self.custom_path.trim().trim_matches('\\');
        let (hive, path) = if typed.is_empty() {
            (self.custom_hive, String::new())
        } else if let Some(hive) = reg_file::parse_hive(typed) {
            (hive, String::new())
        } else if let Some((hive, path)) = reg_file::parse_key_path(typed) {
            (hive, path)
        } else {
            (self.custom_hive, typed.to_owned())
        };

        if !path.is_empty() && registry_editor::open_key(hive, &path).is_err() {
            self.set_error(format!(
                "Раздел {} не найден.",
                registry_editor::full_path(hive, &path)
            ));
            return;
        }

        let label = if path.is_empty() {
            "Весь улей".to_owned()
        } else {
            "Своя ветка".to_owned()
        };
        let root = SnapshotRoot {
            hive: hive.short().to_owned(),
            path,
            label,
        };
        let identity = root.display();
        if self
            .roots
            .iter()
            .chain(self.custom_roots.iter())
            .any(|existing| existing.display() == identity)
        {
            self.selected_roots.insert(identity);
            self.custom_path.clear();
            self.set_ok("Эта ветка уже в списке и отмечена.".to_owned());
            return;
        }

        self.selected_roots.insert(identity);
        self.custom_roots.push(root);
        self.custom_path.clear();
        self.message = None;
    }

    pub fn remove_custom_root(&mut self, root: &SnapshotRoot) {
        if self.is_busy() {
            return;
        }
        let identity = root.display();
        self.custom_roots
            .retain(|existing| existing.display() != identity);
        self.selected_roots.remove(&identity);
    }

    pub fn create(&mut self) {
        if self.is_busy() {
            return;
        }
        let mut seen = HashSet::new();
        let roots: Vec<SnapshotRoot> = self
            .roots
            .iter()
            .chain(self.custom_roots.iter())
            .filter(|root| self.is_root_selected(root))
            .filter(|root| seen.insert(root.display()))
            .cloned()
            .collect();
        if roots.is_empty() {
            self.set_error("Выберите хотя бы один раздел для снимка.".to_owned());
            return;
        }

        let name = self.name.clone();
        self.busy = Some(Busy::Create);
        self.job = Some(Job::spawn(
            "chipclean-snapshot-create",
            move |_cancel, tx| {
                let result = registry_snapshots::create_snapshot(&name, &roots)
                    .map_err(|err| err.to_string());
                let _ = tx.send(Event::Created(result));
            },
        ));
    }

    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    pub fn busy_label(&self) -> Option<&'static str> {
        self.busy.map(|busy| match busy {
            Busy::Create => "Создание снимка…",
            Busy::Compare => "Сравнение снимка с реестром…",
        })
    }

    pub fn poll(&mut self) {
        let (events, disconnected) = job::drain(&self.job);
        let mut finished = false;
        for event in events {
            match event {
                Event::Created(result) => {
                    self.created(result);
                    finished = true;
                }
                Event::Compared(result) => {
                    self.compared(result);
                    finished = true;
                }
            }
        }

        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.job.take() {
            job.finish();
        }
        self.busy = None;
        if !finished {
            self.set_error("Операция прервана: поток завершился неожиданно.".to_owned());
        }
    }

    fn created(&mut self, result: Result<SnapshotMeta, String>) {
        match result {
            Ok(meta) => {
                let label = meta.name.clone();
                self.name.clear();
                self.diff = None;
                self.reload();
                self.selected = Some(meta.id);
                self.set_ok(format!("Снимок создан: {label}"));
            }
            Err(err) => self.set_error(err),
        }
    }

    pub fn select(&mut self, id: String) {
        if self.is_busy() {
            return;
        }
        self.selected = Some(id);
        self.diff = None;
    }

    pub fn compare(&mut self) {
        if self.is_busy() {
            return;
        }
        let Some(meta) = self.selected_meta().cloned() else {
            self.set_error("Выберите снимок.".to_owned());
            return;
        };

        self.busy = Some(Busy::Compare);
        self.job = Some(Job::spawn(
            "chipclean-snapshot-compare",
            move |_cancel, tx| {
                let result = registry_snapshots::compare(&meta).map_err(|err| err.to_string());
                let _ = tx.send(Event::Compared(result));
            },
        ));
    }

    fn compared(&mut self, result: Result<SnapshotDiff, String>) {
        match result {
            Ok(diff) => {
                let count = diff.added_keys.len()
                    + diff.removed_keys.len()
                    + diff.added_values.len()
                    + diff.removed_values.len()
                    + diff.changed_values.len();
                self.message = Some(Message {
                    text: if diff.is_empty() {
                        "Изменений не найдено.".to_owned()
                    } else {
                        format!("Найдено изменений: {count}")
                    },
                    is_error: false,
                });
                self.diff = Some(diff);
            }
            Err(err) => self.set_error(err),
        }
    }

    pub fn request_delete(&mut self) {
        if self.is_busy() {
            return;
        }
        if let Some(meta) = self.selected_meta().cloned() {
            self.pending_delete = Some(meta);
        }
    }

    pub fn cancel_delete(&mut self) {
        self.pending_delete = None;
    }

    pub fn confirm_delete(&mut self) {
        let Some(meta) = self.pending_delete.take() else {
            return;
        };
        if self.is_busy() {
            return;
        }
        match registry_snapshots::delete_snapshot(&meta) {
            Ok(()) => {
                self.diff = None;
                self.selected = None;
                self.reload();
                self.set_ok("Снимок удалён.".to_owned());
            }
            Err(err) => self.set_error(err.to_string()),
        }
    }

    pub fn selected_meta(&self) -> Option<&SnapshotMeta> {
        let id = self.selected.as_ref()?;
        self.snapshots.iter().find(|meta| &meta.id == id)
    }

    pub fn snapshot_size(&self, meta: &SnapshotMeta) -> u64 {
        registry_snapshots::snapshot_size(meta).unwrap_or(0)
    }

    fn set_ok(&mut self, text: String) {
        self.message = Some(Message {
            text,
            is_error: false,
        });
    }

    fn set_error(&mut self, text: String) {
        self.message = Some(Message {
            text,
            is_error: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_hive_custom_root_uses_an_empty_path() {
        let mut state = SnapshotState::new();
        state.custom_hive = EditorHive::Hklm;
        state.add_custom_root();

        assert_eq!(state.custom_roots.len(), 1);
        assert_eq!(state.custom_roots[0].display(), "HKLM");
        assert!(state.is_root_selected(&state.custom_roots[0]));
    }

    #[test]
    fn typed_hive_name_means_the_whole_hive() {
        let mut state = SnapshotState::new();
        state.custom_hive = EditorHive::Hklm;
        state.custom_path = "HKEY_CURRENT_USER".to_owned();
        state.add_custom_root();

        assert_eq!(state.custom_roots.len(), 1);
        assert_eq!(state.custom_roots[0].display(), "HKCU");
        assert_eq!(state.custom_roots[0].label, "Весь улей");
    }

    #[test]
    fn custom_root_accepts_a_full_path_and_deduplicates() {
        let _guard = crate::core::registry_test_guard();
        use windows_registry::CURRENT_USER;

        let unique = format!(
            "ChipCleanSnapshotRoot-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        );
        let path = format!(r"Software\{unique}");
        let key = CURRENT_USER.create(&path).unwrap();
        drop(key);

        let mut state = SnapshotState::new();
        state.custom_hive = EditorHive::Hklm;
        state.custom_path = format!("HKCU\\{path}");
        state.add_custom_root();
        assert_eq!(state.custom_roots.len(), 1);
        assert_eq!(state.custom_roots[0].hive, "HKCU");
        assert_eq!(state.custom_roots[0].path, path);

        state.custom_path = format!("HKCU\\{path}");
        state.add_custom_root();
        assert_eq!(state.custom_roots.len(), 1);

        let root = state.custom_roots[0].clone();
        state.remove_custom_root(&root);
        assert!(state.custom_roots.is_empty());
        assert!(!state.is_root_selected(&root));

        let _ = CURRENT_USER.remove_tree(&path);
    }

    #[test]
    fn rejects_a_missing_custom_root() {
        let mut state = SnapshotState::new();
        state.custom_path = r"Software\ChipCleanMissingRoot-xyz".to_owned();
        state.add_custom_root();

        assert!(state.custom_roots.is_empty());
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|message| message.is_error)
        );
    }
}
