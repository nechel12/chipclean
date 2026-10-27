use std::collections::HashSet;

use crate::core::{
    job::{self, Job},
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

    pub fn create(&mut self) {
        if self.is_busy() {
            return;
        }
        let roots: Vec<SnapshotRoot> = self
            .roots
            .iter()
            .filter(|root| self.is_root_selected(root))
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
