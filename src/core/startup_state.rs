use super::startup::{self, StartupEntry};

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

pub struct StartupState {
    pub entries: Vec<StartupEntry>,
    pub loaded: bool,
    pub message: Option<Message>,
}

impl StartupState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            loaded: false,
            message: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.reload();
        }
    }

    pub fn reload(&mut self) {
        match startup::scan_entries() {
            Ok(entries) => {
                self.entries = entries;
                self.loaded = true;
            }
            Err(err) => {
                self.loaded = true;
                self.message = Some(Message {
                    text: format!("Не удалось загрузить автозагрузку: {err}"),
                    is_error: true,
                });
            }
        }
    }

    pub fn refresh(&mut self) {
        self.reload();
    }

    pub fn toggle(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index).cloned() else {
            return;
        };

        match startup::toggle_entry(&entry) {
            Ok(message) => {
                self.message = Some(Message {
                    text: message,
                    is_error: false,
                });
                self.reload();
            }
            Err(err) => {
                self.message = Some(Message {
                    text: err.to_string(),
                    is_error: true,
                });
                self.reload();
            }
        }
    }

    pub fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.enabled).count()
    }

    pub fn disabled_count(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.enabled).count()
    }
}
