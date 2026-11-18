use crate::core::{
    job::{self, Job},
    rollback::{self, Inventory, RegistryBackup},
    startup::StartupEntry,
    traces::QuarantinedTrace,
};

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

#[derive(Clone)]
pub enum RestoreRequest {
    Registry(RegistryBackup),
    Trace(QuarantinedTrace),
    Startup(StartupEntry),
}

impl RestoreRequest {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Registry(_) => ".reg-бэкап",
            Self::Trace(_) => "карантин следов",
            Self::Startup(_) => "автозагрузка",
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Registry(backup) => backup.label.clone(),
            Self::Trace(entry) => entry.title.clone(),
            Self::Startup(entry) => entry.title.clone(),
        }
    }

    pub fn details(&self) -> Vec<String> {
        match self {
            Self::Registry(backup) => {
                let mut lines = vec![
                    format!("Каталог: {}", backup.group),
                    format!("Разделов в файле: {}", backup.targets.len()),
                ];
                for target in backup.targets.iter().take(8) {
                    lines.push(format!("{}\\{}", target.hive.short(), target.path));
                }
                if backup.targets.len() > 8 {
                    lines.push(format!("… и ещё {}", backup.targets.len() - 8));
                }
                lines
            }
            Self::Trace(entry) => vec![
                format!("Исходный путь: {}", entry.original_path.display()),
                format!("Карантин: {}", entry.stored_path.display()),
                format!("Дата: {}", entry.quarantined_at),
            ],
            Self::Startup(entry) => vec![
                format!("Источник: {}", entry.source.label()),
                format!("Команда: {}", entry.command),
            ],
        }
    }

    pub fn needs_ack(&self) -> bool {
        matches!(self, Self::Registry(_))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Scan,
    Restore,
}

enum Event {
    Scanned(Result<Inventory, String>),
    Restored(Result<String, String>),
}

pub struct RollbackState {
    pub inventory: Inventory,
    pub loaded: bool,
    pub query: String,
    pub message: Option<Message>,
    pub pending: Option<RestoreRequest>,
    pub confirm_ack: bool,
    job: Option<Job<Event>>,
    busy: Option<Busy>,
}

impl RollbackState {
    pub fn create_restore_point(&mut self) {
        match crate::core::restore_point::create("ChipClean: перед операциями отката")
        {
            Ok(()) => {
                self.message = Some(Message {
                    text: "Точка восстановления Windows создана.".to_owned(),
                    is_error: false,
                });
            }
            Err(err) => {
                self.message = Some(Message {
                    text: format!("Не удалось создать точку восстановления: {err}"),
                    is_error: true,
                });
            }
        }
    }

    pub fn new() -> Self {
        Self {
            inventory: Inventory::default(),
            loaded: false,
            query: String::new(),
            message: None,
            pending: None,
            confirm_ack: false,
            job: None,
            busy: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.loaded = true;
            self.start_scan();
        }
    }

    pub fn start_scan(&mut self) {
        if self.is_busy() {
            return;
        }
        self.busy = Some(Busy::Scan);
        self.job = Some(Job::spawn("chipclean-rollback-scan", |_cancel, tx| {
            let _ = tx.send(Event::Scanned(Ok(rollback::scan_inventory())));
        }));
    }

    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    pub fn busy_label(&self) -> Option<&'static str> {
        self.busy.map(|busy| match busy {
            Busy::Scan => "Сбор данных для отката…",
            Busy::Restore => "Восстановление…",
        })
    }

    pub fn poll(&mut self) {
        let (events, disconnected) = job::drain(&self.job);
        let mut finished = false;
        let mut rescan = false;

        for event in events {
            finished = true;
            match event {
                Event::Scanned(result) => match result {
                    Ok(inventory) => {
                        self.inventory = inventory;
                        self.loaded = true;
                    }
                    Err(err) => self.set_error(err),
                },
                Event::Restored(result) => {
                    match result {
                        Ok(text) => self.set_ok(text),
                        Err(err) => self.set_error(err),
                    }
                    rescan = true;
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
        if rescan {
            self.start_scan();
        }
    }

    pub fn request_registry(&mut self, backup: &RegistryBackup) {
        if self.is_busy() {
            return;
        }
        self.pending = Some(RestoreRequest::Registry(backup.clone()));
        self.confirm_ack = false;
    }

    pub fn request_trace(&mut self, entry: &QuarantinedTrace) {
        if self.is_busy() {
            return;
        }
        self.pending = Some(RestoreRequest::Trace(entry.clone()));
        self.confirm_ack = false;
    }

    pub fn request_startup(&mut self, entry: &StartupEntry) {
        if self.is_busy() {
            return;
        }
        self.pending = Some(RestoreRequest::Startup(entry.clone()));
        self.confirm_ack = false;
    }

    pub fn cancel_restore(&mut self) {
        self.pending = None;
        self.confirm_ack = false;
    }

    pub fn confirm_restore(&mut self) {
        let Some(request) = self.pending.clone() else {
            return;
        };
        if request.needs_ack() && !self.confirm_ack {
            self.set_error("Подтвердите восстановление из .reg-бэкапа.".to_owned());
            return;
        }
        if self.is_busy() {
            return;
        }

        self.pending = None;
        self.confirm_ack = false;
        self.busy = Some(Busy::Restore);

        self.job = Some(Job::spawn(
            "chipclean-rollback-restore",
            move |_cancel, tx| {
                let result = match request {
                    RestoreRequest::Registry(backup) => rollback::restore_registry(&backup)
                        .map(|report| {
                            if report.safety_backups.is_empty() {
                                "Реестр восстановлен. Страховые копии не потребовались.".to_owned()
                            } else {
                                format!(
                                    "Реестр восстановлен. Страховых копий: {}",
                                    report.safety_backups.len()
                                )
                            }
                        })
                        .map_err(|err| format!("Не удалось восстановить реестр: {err:#}")),
                    RestoreRequest::Trace(entry) => rollback::restore_trace(&entry)
                        .map_err(|err| format!("Не удалось вернуть объект: {err:#}")),
                    RestoreRequest::Startup(entry) => rollback::restore_startup(&entry)
                        .map_err(|err| format!("Не удалось включить автозагрузку: {err:#}")),
                };
                let _ = tx.send(Event::Restored(result));
            },
        ));
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
