use std::collections::{HashMap, HashSet};

use crate::{
    config,
    core::{
        job::{self, Job},
        registry_editor::{
            self, EditorHive, KeyStats, REG_DWORD, REG_SZ, RegValueRow, ReplaceHit, ReplaceReport,
            SearchHit, join_path, split_parent,
        },
        registry_import::{self, ImportPreview, ImportReport},
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

enum ReplaceEvent {
    Found(Result<Vec<ReplaceHit>, String>),
    Replaced(Result<ReplaceReport, String>),
}

enum ImportEvent {
    Previewed(Result<ImportPreview, String>),
    Imported(Result<ImportReport, String>),
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
    Import {
        preview: Option<ImportPreview>,
        error: Option<String>,
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
    pub replace_open: bool,
    pub replace_find: String,
    pub replace_with: String,
    pub replace_ack: bool,
    pub replace_hits: Option<Vec<ReplaceHit>>,
    pub replace_message: Option<Message>,
    pub dialog: Option<EditorDialog>,
    pub message: Option<Message>,
    loaded: bool,
    search_job: Option<Job<SearchEvent>>,
    replace_job: Option<Job<ReplaceEvent>>,
    import_job: Option<Job<ImportEvent>>,
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
            replace_open: false,
            replace_find: String::new(),
            replace_with: String::new(),
            replace_ack: false,
            replace_hits: None,
            replace_message: None,
            dialog: None,
            message: None,
            loaded: false,
            search_job: None,
            replace_job: None,
            import_job: None,
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
        self.abort_replace();
        self.hive = hive;
        self.selected.clear();
        self.selected_value = None;
        self.children.clear();
        self.expanded.clear();
        self.search_hits.clear();
        self.search_open = false;
        self.replace_open = false;
        self.replace_hits = None;
        self.replace_message = None;
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

    pub fn reload(&mut self) {
        if registry_editor::open_key(self.hive, &self.selected).is_err() {
            self.selected = split_parent(&self.selected)
                .map(|(parent, _)| parent)
                .unwrap_or_default();
        }
        self.selected_value = None;
        let hive = self.hive;
        self.expanded
            .retain(|path| path.is_empty() || registry_editor::open_key(hive, path).is_ok());
        self.expanded.insert(String::new());
        self.children.clear();
        self.load_children(String::new());
        let mut paths: Vec<String> = self
            .expanded
            .iter()
            .filter(|path| !path.is_empty())
            .cloned()
            .collect();
        paths.sort();
        for path in paths {
            self.load_children(path);
        }
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

    pub fn is_replace_busy(&self) -> bool {
        self.replace_job.is_some()
    }

    pub fn is_importing(&self) -> bool {
        self.import_job.is_some()
    }

    pub fn is_busy(&self) -> bool {
        self.is_searching() || self.is_replace_busy() || self.is_importing()
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

    pub fn open_replace(&mut self) {
        self.replace_open = true;
        self.replace_hits = None;
        self.replace_ack = false;
        self.replace_message = None;
    }

    pub fn close_replace(&mut self) {
        self.replace_open = false;
        self.replace_hits = None;
        self.replace_ack = false;
        self.replace_message = None;
        self.abort_replace();
    }

    pub fn find_replacements(&mut self) {
        let find = self.replace_find.trim().to_owned();
        if find.is_empty() {
            self.replace_hits = None;
            self.replace_message = Some(Message {
                text: "Укажите строку поиска.".to_owned(),
                is_error: true,
            });
            return;
        }

        self.abort_replace();
        self.replace_hits = None;
        self.replace_ack = false;
        self.replace_message = Some(Message {
            text: "Поиск совпадений…".to_owned(),
            is_error: false,
        });

        let hive = self.hive;
        let root = self.selected.clone();
        let replacement = self.replace_with.clone();
        self.replace_job = Some(Job::spawn(
            "chipclean-registry-replace-scan",
            move |cancel, tx| {
                let result = registry_editor::find_replacements_cancellable(
                    hive,
                    &root,
                    &find,
                    &replacement,
                    500,
                    &cancel,
                )
                .map_err(|err| err.to_string());
                let _ = tx.send(ReplaceEvent::Found(result));
            },
        ));
    }

    pub fn replace_all(&mut self) {
        let Some(hits) = self.replace_hits.clone() else {
            return;
        };
        if hits.is_empty() || self.replace_job.is_some() {
            return;
        }
        if !self.replace_ack {
            self.replace_message = Some(Message {
                text: "Подтвердите замену: без .reg-бэкапа изменения не откатить.".to_owned(),
                is_error: true,
            });
            return;
        }

        self.replace_message = Some(Message {
            text: "Замена значений…".to_owned(),
            is_error: false,
        });
        self.replace_job = Some(Job::spawn(
            "chipclean-registry-replace",
            move |_cancel, tx| {
                let result = registry_editor::replace_values(&hits, &config::backups_dir())
                    .map_err(|err| err.to_string());
                let _ = tx.send(ReplaceEvent::Replaced(result));
            },
        ));
    }

    pub fn poll_replace(&mut self) {
        let (events, disconnected) = job::drain(&self.replace_job);
        let mut finished = false;
        for event in events {
            finished = true;
            match event {
                ReplaceEvent::Found(result) => match result {
                    Ok(hits) => {
                        self.replace_message = Some(Message {
                            text: if hits.is_empty() {
                                "Совпадений не найдено.".to_owned()
                            } else {
                                format!("Найдено значений: {}", hits.len())
                            },
                            is_error: false,
                        });
                        self.replace_hits = Some(hits);
                    }
                    Err(err) => {
                        self.replace_hits = None;
                        self.replace_message = Some(Message {
                            text: err,
                            is_error: true,
                        });
                    }
                },
                ReplaceEvent::Replaced(result) => match result {
                    Ok(report) => {
                        self.replace_hits = None;
                        self.replace_ack = false;
                        self.replace_message = Some(Message {
                            text: format!(
                                "Заменено значений: {}. Бэкапов: {}.",
                                report.replaced, report.backups
                            ),
                            is_error: false,
                        });
                        self.load_values();
                    }
                    Err(err) => {
                        self.replace_message = Some(Message {
                            text: err,
                            is_error: true,
                        });
                    }
                },
            }
        }

        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.replace_job.take() {
            job.finish();
        }
        if !finished {
            self.replace_message = Some(Message {
                text: "Операция прервана: поток завершился неожиданно.".to_owned(),
                is_error: true,
            });
        }
    }

    fn abort_replace(&mut self) {
        if let Some(job) = self.replace_job.take() {
            job.cancel();
        }
    }

    pub fn open_import_dialog(&mut self) {
        let Some(path) = crate::win::file_dialog::open_reg_file() else {
            return;
        };
        self.abort_import();
        self.dialog = Some(EditorDialog::Import {
            preview: None,
            error: None,
        });
        self.import_job = Some(Job::spawn(
            "chipclean-registry-import-preview",
            move |_cancel, tx| {
                let result = registry_import::preview(&path).map_err(|err| err.to_string());
                let _ = tx.send(ImportEvent::Previewed(result));
            },
        ));
    }

    pub fn submit_import(&mut self) {
        if self.import_job.is_some() {
            return;
        }
        let Some(EditorDialog::Import {
            preview: Some(preview),
            error: None,
        }) = &self.dialog
        else {
            return;
        };
        let preview = preview.clone();
        self.import_job = Some(Job::spawn(
            "chipclean-registry-import",
            move |_cancel, tx| {
                let result = registry_import::import(&preview).map_err(|err| err.to_string());
                let _ = tx.send(ImportEvent::Imported(result));
            },
        ));
    }

    pub fn poll_import(&mut self) {
        let (events, disconnected) = job::drain(&self.import_job);
        let mut finished = false;
        for event in events {
            finished = true;
            match event {
                ImportEvent::Previewed(result) => match result {
                    Ok(preview) => {
                        self.dialog = Some(EditorDialog::Import {
                            preview: Some(preview),
                            error: None,
                        });
                    }
                    Err(err) => {
                        self.dialog = Some(EditorDialog::Import {
                            preview: None,
                            error: Some(err),
                        });
                    }
                },
                ImportEvent::Imported(result) => match result {
                    Ok(report) => {
                        let (keys, values) = match &self.dialog {
                            Some(EditorDialog::Import {
                                preview: Some(preview),
                                ..
                            }) => (preview.key_count(), preview.value_count),
                            _ => (0, 0),
                        };
                        self.dialog = None;
                        self.reload();
                        self.set_ok(format!(
                            "Импорт завершён: разделов {keys}, значений {values}. Бэкапов: {}.",
                            report.backups
                        ));
                    }
                    Err(err) => {
                        if let Some(EditorDialog::Import { error, .. }) = &mut self.dialog {
                            *error = Some(err);
                        } else {
                            self.dialog = Some(EditorDialog::Import {
                                preview: None,
                                error: Some(err),
                            });
                        }
                    }
                },
            }
        }

        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.import_job.take() {
            job.finish();
        }
        if !finished {
            let text = "Импорт прерван: поток завершился неожиданно.".to_owned();
            if let Some(EditorDialog::Import { error, .. }) = &mut self.dialog {
                *error = Some(text);
            } else {
                self.set_error(text);
            }
        }
    }

    fn abort_import(&mut self) {
        if let Some(job) = self.import_job.take() {
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
        self.abort_import();
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
            EditorDialog::Import { preview, error } => {
                self.dialog = Some(EditorDialog::Import { preview, error });
                self.submit_import();
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
