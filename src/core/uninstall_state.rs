use super::{
    job::{self, Job},
    tracked_uninstall::{self, RegistrySnapshotEntry},
    uninstall::{self, ProgramEntry},
};

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
    pub last_launched: Option<String>,
    pub snapshot_requested: bool,
    pub tracked_before: Option<Vec<RegistrySnapshotEntry>>,
    pub tracked_results: Option<Vec<RegistrySnapshotEntry>>,
    pub tracked_pending_clean: bool,
    traces_request: Option<String>,
    job: Option<Job<Result<Vec<ProgramEntry>, String>>>,
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
            last_launched: None,
            snapshot_requested: false,
            tracked_before: None,
            tracked_results: None,
            tracked_pending_clean: false,
            traces_request: None,
            job: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.reload();
        }
    }

    pub fn reload(&mut self) {
        if self.job.is_some() {
            return;
        }
        self.job = Some(Job::spawn("chipclean-programs-scan", |_cancel, tx| {
            let _ = tx.send(uninstall::scan_programs().map_err(|err| err.to_string()));
        }));
    }

    pub fn poll(&mut self) {
        let (events, disconnected) = job::drain(&self.job);
        let mut finished = false;
        for event in events {
            match event {
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
            finished = true;
        }
        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.job.take() {
            job.finish();
        }
        if !finished {
            self.loaded = true;
            self.message = Some(Message {
                text: "Не удалось загрузить программы: поток завершился.".to_owned(),
                is_error: true,
            });
        }
    }

    pub fn is_loading(&self) -> bool {
        self.job.is_some()
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
        if self.is_loading() {
            return;
        }
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

        if self.snapshot_requested {
            self.tracked_before = Some(tracked_uninstall::capture_registry_snapshot());
        }
        self.snapshot_requested = false;
        self.tracked_results = None;
        self.tracked_pending_clean = false;

        match uninstall::launch_program(entry) {
            Ok(message) => {
                self.last_launched = Some(entry.display_name.clone());
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

    /// Сравнивает снимок до/после: ищет исчезнувшие ключи реестра.
    pub fn finish_tracked(&mut self) {
        let Some(before) = self.tracked_before.take() else {
            return;
        };
        let removed = tracked_uninstall::diff_removed(&before);
        if removed.is_empty() {
            self.message = Some(Message {
                text: "Удалённых ключей не обнаружено.".to_owned(),
                is_error: false,
            });
        }
        self.tracked_results = Some(removed);
    }

    pub fn request_clean_traces(&mut self) {
        if self.tracked_results.as_ref().is_some_and(|items| !items.is_empty()) {
            self.tracked_pending_clean = true;
        }
    }

    pub fn cancel_clean_traces(&mut self) {
        self.tracked_pending_clean = false;
    }

    /// Удаляет найденные ключи-следы с записью в журнал.
    pub fn confirm_clean_traces(&mut self) {
        let Some(items) = self.tracked_results.take() else {
            return;
        };
        self.tracked_pending_clean = false;

        let Ok(mut journal) = crate::core::journal::Journal::open() else {
            self.message = Some(Message {
                text: "Не удалось открыть журнал операций.".to_owned(),
                is_error: true,
            });
            return;
        };

        let mut removed = 0usize;
        let mut errors = 0usize;
        for entry in &items {
            match tracked_uninstall::remove_entry(entry) {
                Ok(()) => {
                    removed += 1;
                    let _ = journal.record(serde_json::json!({
                        "ts": chrono::Utc::now().to_rfc3339(),
                        "event": "uninstall_trace_removed",
                        "hive": entry.hive,
                        "path": entry.path,
                    }));
                }
                Err(err) => {
                    errors += 1;
                    tracing::warn!(%err, path = %entry.display(), "failed to remove trace key");
                }
            }
        }
        let _ = journal.flush();

        self.message = Some(Message {
            text: if errors == 0 {
                format!("Удалено ключей-следов: {removed}.")
            } else {
                format!("Удалено {removed}, ошибок: {errors}.")
            },
            is_error: errors > 0,
        });
    }

    pub fn request_traces_scan(&mut self) {
        if let Some(program) = self.last_launched.clone() {
            self.traces_request = Some(program);
        }
    }

    pub fn take_traces_request(&mut self) -> Option<String> {
        self.traces_request.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_request_is_taken_once() {
        let mut state = UninstallState::new();
        state.last_launched = Some("Test App".to_owned());

        state.request_traces_scan();

        assert_eq!(state.take_traces_request().as_deref(), Some("Test App"));
        assert!(state.take_traces_request().is_none());
    }
}
