use super::uninstall::{self, ProgramEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramSort {
    Name,
    Publisher,
    Size,
}

impl ProgramSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "По имени",
            Self::Publisher => "По издателю",
            Self::Size => "По размеру",
        }
    }
}

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

pub struct UninstallState {
    pub entries: Vec<ProgramEntry>,
    pub loaded: bool,
    pub query: String,
    pub sort: ProgramSort,
    pub message: Option<Message>,
    pub pending_uninstall: Option<usize>,
}

impl UninstallState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            loaded: false,
            query: String::new(),
            sort: ProgramSort::Name,
            message: None,
            pending_uninstall: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.reload();
        }
    }

    pub fn reload(&mut self) {
        match uninstall::scan_programs() {
            Ok(entries) => {
                self.entries = entries;
                self.loaded = true;
            }
            Err(err) => {
                self.loaded = true;
                self.message = Some(Message {
                    text: format!("Не удалось загрузить программы: {err}"),
                    is_error: true,
                });
            }
        }
    }

    pub fn refresh(&mut self) {
        self.reload();
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        let mut indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                query.is_empty()
                    || entry.display_name.to_lowercase().contains(&query)
                    || entry.publisher.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();

        indices.sort_by(|left, right| {
            let left = &self.entries[*left];
            let right = &self.entries[*right];

            match self.sort {
                ProgramSort::Name => left
                    .display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase()),
                ProgramSort::Publisher => left
                    .publisher
                    .to_lowercase()
                    .cmp(&right.publisher.to_lowercase())
                    .then_with(|| {
                        left.display_name
                            .to_lowercase()
                            .cmp(&right.display_name.to_lowercase())
                    }),
                ProgramSort::Size => right.size_kb.cmp(&left.size_kb).then_with(|| {
                    left.display_name
                        .to_lowercase()
                        .cmp(&right.display_name.to_lowercase())
                }),
            }
        });

        indices
    }

    pub fn request_uninstall(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };

        if entry.command.is_none() {
            self.message = Some(Message {
                text: "Для этой программы не найден штатный деинсталлятор.".to_owned(),
                is_error: true,
            });
            return;
        }

        self.pending_uninstall = Some(index);
    }

    pub fn cancel_uninstall(&mut self) {
        self.pending_uninstall = None;
    }

    pub fn confirm_uninstall(&mut self) {
        let Some(index) = self.pending_uninstall else {
            return;
        };
        self.pending_uninstall = None;

        let Some(entry) = self.entries.get(index) else {
            return;
        };

        match uninstall::launch_program(entry) {
            Ok(message) => {
                self.message = Some(Message {
                    text: message,
                    is_error: false,
                });
            }
            Err(err) => {
                self.message = Some(Message {
                    text: format!("Не удалось запустить деинсталлятор: {err}"),
                    is_error: true,
                });
            }
        }
    }
}
