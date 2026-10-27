use std::collections::{HashMap, HashSet};

use crate::{
    config,
    core::{
        job::{self, Job},
        registry_editor::{
            self, EditorHive, KeyStats, REG_DWORD, REG_SZ, RegValueRow, SearchHit, join_path,
            split_parent,
        },
    },
};

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

enum SearchEvent {
    Finished {
        hive: EditorHive,
        query: String,
        result: Result<Vec<SearchHit>, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueMode {
    New,
    Edit,
}

pub enum EditorDialog {
    CreateKey {
        name: String,
        error: Option<String>,
    },
    Value {
        mode: ValueMode,
        original_name: String,
        name: String,
        ty: u32,
        text: String,
        error: Option<String>,
    },
    DeleteValue {
        name: String,
    },
    DeleteKey {
        path: String,
        stats: KeyStats,
    },
}

pub struct EditorState {
    pub hive: EditorHive,
    pub selected: String,
    pub selected_value: Option<String>,
    pub expanded: HashSet<String>,
    pub children: HashMap<String, Vec<String>>,
    pub values: Vec<RegValueRow>,
    pub search_query: String,
    pub search_hits: Vec<SearchHit>,
    pub search_open: bool,
    pub dialog: Option<EditorDialog>,
    pub message: Option<Message>,
    loaded: bool,
    search_job: Option<Job<SearchEvent>>,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            hive: EditorHive::Hkcu,
            selected: String::new(),
            selected_value: None,
            expanded: HashSet::new(),
            children: HashMap::new(),
            values: Vec::new(),
            search_query: String::new(),
            search_hits: Vec::new(),
            search_open: false,
            dialog: None,
            message: None,
            loaded: false,
            search_job: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.expanded.insert(String::new());
        self.load_children(String::new());
        self.load_values();
    }

    pub fn set_hive(&mut self, hive: EditorHive) {
        if self.hive == hive {
            return;
        }
        self.abort_search();
        self.hive = hive;
        self.selected.clear();
        self.selected_value = None;
        self.children.clear();
        self.expanded.clear();
        self.search_hits.clear();
        self.search_open = false;
        self.expanded.insert(String::new());
        self.load_children(String::new());
        self.load_values();
    }

    pub fn refresh(&mut self) {
        self.abort_search();
        self.children.clear();
        self.expanded.clear();
        self.expanded.insert(String::new());
        self.load_children(String::new());
        self.load_values();
    }

    pub fn select(&mut self, path: String) {
        self.selected = path;
        self.selected_value = None;
        self.load_values();
    }

    pub fn select_value(&mut self, name: String) {
        self.selected_value = Some(name);
    }

    pub fn expand(&mut self, path: String) {
        if !self.expanded.insert(path.clone()) {
            return;
        }
        if !self.children.contains_key(&path) {
            self.load_children(path);
        }
    }

    pub fn collapse(&mut self, path: &str) {
        self.expanded.remove(path);
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    fn load_children(&mut self, path: String) {
        match registry_editor::list_subkeys(self.hive, &path) {
            Ok(children) => {
                self.children.insert(path, children);
            }
            Err(err) => self.set_error(err.to_string()),
        }
    }

    fn load_values(&mut self) {
        match registry_editor::list_values(self.hive, &self.selected) {
            Ok(values) => {
                self.values = values;
                if self
                    .selected_value
                    .as_ref()
                    .is_some_and(|selected| !self.values.iter().any(|row| &row.name == selected))
                {
                    self.selected_value = None;
                }
            }
            Err(err) => {
                self.values.clear();
                self.set_error(err.to_string());
            }
        }
    }

    pub fn search(&mut self) {
        let query = self.search_query.trim().to_owned();
        if query.is_empty() {
            self.abort_search();
            self.search_hits.clear();
            self.search_open = false;
            return;
        }

        self.abort_search();
        let hive = self.hive;
        self.message = Some(Message {
            text: "Поиск…".to_owned(),
            is_error: false,
        });
        self.search_job = Some(Job::spawn(
            "chipclean-registry-search",
            move |cancel, tx| {
                let result = registry_editor::search_cancellable(hive, "", &query, 200, &cancel)
                    .map_err(|err| err.to_string());
                let _ = tx.send(SearchEvent::Finished {
                    hive,
                    query,
                    result,
                });
            },
        ));
    }

    pub fn is_searching(&self) -> bool {
        self.search_job.is_some()
    }

    pub fn poll_search(&mut self) {
        let (events, disconnected) = job::drain(&self.search_job);
        let mut finished = false;
        for event in events {
            match event {
                SearchEvent::Finished {
                    hive,
                    query,
                    result,
                } => {
                    finished = true;
                    if hive != self.hive || query != self.search_query.trim() {
                        continue;
                    }
                    match result {
                        Ok(hits) => {
                            self.search_open = true;
                            self.message = Some(Message {
                                text: if hits.is_empty() {
                                    "Ничего не найдено.".to_owned()
                                } else {
                                    format!("Найдено: {}", hits.len())
                                },
                                is_error: false,
                            });
                            self.search_hits = hits;
                        }
                        Err(err) => {
                            self.search_open = false;
                            self.set_error(err);
                        }
                    }
                }
            }
        }

        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.search_job.take() {
            job.finish();
        }
        if !finished {
            self.search_open = false;
            self.set_error("Поиск прерван: поток завершился неожиданно.".to_owned());
        }
    }

    fn abort_search(&mut self) {
        if let Some(job) = self.search_job.take() {
            job.cancel();
        }
    }

    pub fn open_search_hit(&mut self, hit: SearchHit) {
        self.expand_ancestors(&hit.path);
        self.selected = hit.path;
        self.selected_value = hit.value_name;
        self.load_values();
        self.search_open = false;
    }

    fn expand_ancestors(&mut self, path: &str) {
        self.expanded.insert(String::new());
        if !self.children.contains_key("") {
            self.load_children(String::new());
        }
        let parts: Vec<&str> = path.split('\\').filter(|part| !part.is_empty()).collect();
        let mut current = String::new();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            current = join_path(&current, part);
            self.expanded.insert(current.clone());
            if !self.children.contains_key(&current) {
                self.load_children(current.clone());
            }
        }
    }

    pub fn open_create_key(&mut self) {
        self.dialog = Some(EditorDialog::CreateKey {
            name: String::new(),
            error: None,
        });
    }

    pub fn open_new_value(&mut self) {
        self.dialog = Some(EditorDialog::Value {
            mode: ValueMode::New,
            original_name: String::new(),
            name: String::new(),
            ty: REG_SZ,
            text: String::new(),
            error: None,
        });
    }

    pub fn open_edit_value(&mut self, name: String) {
        match registry_editor::read_value_text(self.hive, &self.selected, &name) {
            Ok((ty, text)) => {
                self.dialog = Some(EditorDialog::Value {
                    mode: ValueMode::Edit,
                    original_name: name.clone(),
                    name,
                    ty,
                    text,
                    error: None,
                });
            }
            Err(err) => self.set_error(err.to_string()),
        }
    }

    pub fn open_delete_value(&mut self, name: String) {
        self.dialog = Some(EditorDialog::DeleteValue { name });
    }

    pub fn open_delete_key(&mut self) {
        if self.selected.is_empty() {
            self.set_error("Корневой раздел удалить нельзя.".to_owned());
            return;
        }
        match registry_editor::key_stats(
            self.hive,
            &self.selected,
            registry_editor::KEY_STATS_LIMIT,
        ) {
            Ok(stats) => {
                self.dialog = Some(EditorDialog::DeleteKey {
                    path: self.selected.clone(),
                    stats,
                });
            }
            Err(err) => self.set_error(err.to_string()),
        }
    }

    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }

    pub fn submit_dialog(&mut self) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };

        match dialog {
            EditorDialog::CreateKey { name, .. } => {
                match registry_editor::create_key(
                    self.hive,
                    &self.selected,
                    &name,
                    &config::backups_dir(),
                ) {
                    Ok(backup) => {
                        let path = join_path(&self.selected, name.trim());
                        self.children.remove(&self.selected);
                        self.load_children(self.selected.clone());
                        self.expand(self.selected.clone());
                        self.selected = path;
                        self.selected_value = None;
                        self.load_values();
                        self.set_ok(format!("Раздел создан. Бэкап: {}", backup.display()));
                    }
                    Err(err) => {
                        self.dialog = Some(EditorDialog::CreateKey {
                            name,
                            error: Some(err.to_string()),
                        });
                    }
                }
            }
            EditorDialog::Value {
                mode,
                original_name,
                name,
                ty,
                text,
                ..
            } => {
                let value_name = match mode {
                    ValueMode::New => name.clone(),
                    ValueMode::Edit => original_name,
                };
                match registry_editor::write_value(
                    self.hive,
                    &self.selected,
                    &value_name,
                    ty,
                    &text,
                    &config::backups_dir(),
                ) {
                    Ok(backup) => {
                        self.selected_value = Some(value_name);
                        self.load_values();
                        self.set_ok(format!("Сохранено. Бэкап: {}", backup.display()));
                    }
                    Err(err) => {
                        self.dialog = Some(EditorDialog::Value {
                            mode,
                            original_name: value_name,
                            name,
                            ty,
                            text,
                            error: Some(err.to_string()),
                        });
                    }
                }
            }
            EditorDialog::DeleteValue { name } => {
                match registry_editor::delete_value(
                    self.hive,
                    &self.selected,
                    &name,
                    &config::backups_dir(),
                ) {
                    Ok(backup) => {
                        self.selected_value = None;
                        self.load_values();
                        self.set_ok(format!("Значение удалено. Бэкап: {}", backup.display()));
                    }
                    Err(err) => self.set_error(err.to_string()),
                }
            }
            EditorDialog::DeleteKey { path, .. } => {
                match registry_editor::delete_key(self.hive, &path, &config::backups_dir()) {
                    Ok(backup) => {
                        let parent = split_parent(&path)
                            .map(|(parent, _)| parent)
                            .unwrap_or_default();
                        self.children.remove(&parent);
                        self.select(parent);
                        self.children.remove(&self.selected);
                        self.load_children(self.selected.clone());
                        self.expand(self.selected.clone());
                        self.set_ok(format!("Раздел удалён. Бэкап: {}", backup.display()));
                    }
                    Err(err) => self.set_error(err.to_string()),
                }
            }
        }
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

pub fn value_types() -> [(u32, &'static str); 6] {
    [
        (REG_SZ, "REG_SZ"),
        (registry_editor::REG_EXPAND_SZ, "REG_EXPAND_SZ"),
        (REG_DWORD, "REG_DWORD"),
        (registry_editor::REG_QWORD, "REG_QWORD"),
        (registry_editor::REG_MULTI_SZ, "REG_MULTI_SZ"),
        (registry_editor::REG_BINARY, "REG_BINARY"),
    ]
}
