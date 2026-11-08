use super::{
    job::{self, Job},
    services::{self, ServiceEntry},
    startup::{self, StartupEntry},
    tasks::{self, TaskEntry},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupTab {
    Run,
    Tasks,
    Services,
}

pub struct Message {
    pub text: String,
    pub is_error: bool,
}

enum ToggleEvent {
    Done {
        tab: StartupTab,
        result: Result<String, String>,
    },
}

#[derive(Clone)]
pub struct PendingToggle {
    pub tab: StartupTab,
    pub index: usize,
    pub identifier: String,
    pub title: String,
    pub command: String,
    pub enabling: bool,
    pub manual_restore: bool,
    pub system: bool,
}

pub struct StartupState {
    pub tab: StartupTab,
    pub entries: Vec<StartupEntry>,
    pub tasks: Vec<TaskEntry>,
    pub services: Vec<ServiceEntry>,
    pub loaded_run: bool,
    pub loaded_tasks: bool,
    pub loaded_services: bool,
    pub message: Option<Message>,
    pub pending: Option<PendingToggle>,
    run_job: Option<Job<Result<Vec<StartupEntry>, String>>>,
    tasks_job: Option<Job<Result<Vec<TaskEntry>, String>>>,
    services_job: Option<Job<Result<Vec<ServiceEntry>, String>>>,
    toggle_job: Option<Job<ToggleEvent>>,
}

impl StartupState {
    pub fn new() -> Self {
        Self {
            tab: StartupTab::Run,
            entries: Vec::new(),
            tasks: Vec::new(),
            services: Vec::new(),
            loaded_run: false,
            loaded_tasks: false,
            loaded_services: false,
            message: None,
            pending: None,
            run_job: None,
            tasks_job: None,
            services_job: None,
            toggle_job: None,
        }
    }

    pub fn ensure_loaded(&mut self) {
        if !self.loaded_run {
            self.reload_run();
        }
        if !self.loaded_tasks {
            self.reload_tasks();
        }
        if !self.loaded_services {
            self.reload_services();
        }
    }

    pub fn refresh(&mut self) {
        self.reload_run();
        self.reload_tasks();
        self.reload_services();
    }

    pub fn set_tab(&mut self, tab: StartupTab) {
        if self
            .message
            .as_ref()
            .is_some_and(|message| !message.is_error)
        {
            self.message = None;
        }
        self.tab = tab;
    }

    fn reload_run(&mut self) {
        if self.run_job.is_some() {
            return;
        }
        self.run_job = Some(Job::spawn("chipclean-startup-run", |_cancel, tx| {
            let _ = tx.send(startup::scan_entries().map_err(|err| format!("{err:#}")));
        }));
    }

    fn reload_tasks(&mut self) {
        if self.tasks_job.is_some() {
            return;
        }
        self.tasks_job = Some(Job::spawn("chipclean-startup-tasks", |_cancel, tx| {
            let _ = tx.send(tasks::scan_tasks().map_err(|err| format!("{err:#}")));
        }));
    }

    fn reload_services(&mut self) {
        if self.services_job.is_some() {
            return;
        }
        self.services_job = Some(Job::spawn("chipclean-startup-services", |_cancel, tx| {
            let _ = tx.send(services::scan_services().map_err(|err| format!("{err:#}")));
        }));
    }

    pub fn poll(&mut self) {
        self.poll_run();
        self.poll_tasks();
        self.poll_services();
        self.poll_toggle();
    }

    fn poll_run(&mut self) {
        let (events, disconnected) = job::drain(&self.run_job);
        let mut finished = false;
        for event in events {
            match event {
                Ok(entries) => {
                    self.entries = entries;
                    self.loaded_run = true;
                }
                Err(err) => {
                    self.entries.clear();
                    self.loaded_run = true;
                    self.set_error(format!("Не удалось загрузить автозагрузку: {err}"));
                }
            }
            finished = true;
        }
        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.run_job.take() {
            job.finish();
        }
        if !finished {
            self.loaded_run = true;
            self.set_error("Не удалось загрузить автозагрузку: поток завершился.".to_owned());
        }
    }

    fn poll_tasks(&mut self) {
        let (events, disconnected) = job::drain(&self.tasks_job);
        let mut finished = false;
        for event in events {
            match event {
                Ok(entries) => {
                    self.tasks = entries;
                    self.loaded_tasks = true;
                }
                Err(err) => {
                    self.tasks.clear();
                    self.loaded_tasks = true;
                    self.set_error(format!("Не удалось загрузить задачи планировщика: {err}"));
                }
            }
            finished = true;
        }
        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.tasks_job.take() {
            job.finish();
        }
        if !finished {
            self.loaded_tasks = true;
            self.set_error(
                "Не удалось загрузить задачи планировщика: поток завершился.".to_owned(),
            );
        }
    }

    fn poll_services(&mut self) {
        let (events, disconnected) = job::drain(&self.services_job);
        let mut finished = false;
        for event in events {
            match event {
                Ok(entries) => {
                    tracing::info!(count = entries.len(), "services scanned");
                    self.services = entries;
                    self.loaded_services = true;
                }
                Err(err) => {
                    tracing::warn!(%err, "failed to scan services");
                    self.services.clear();
                    self.loaded_services = true;
                    self.set_error(format!("Не удалось загрузить службы: {err}"));
                }
            }
            finished = true;
        }
        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.services_job.take() {
            job.finish();
        }
        if !finished {
            self.loaded_services = true;
            self.set_error("Не удалось загрузить службы: поток завершился.".to_owned());
        }
    }

    pub fn is_loading(&self) -> bool {
        self.toggle_job.is_some()
            || match self.tab {
                StartupTab::Run => self.run_job.is_some(),
                StartupTab::Tasks => self.tasks_job.is_some(),
                StartupTab::Services => self.services_job.is_some(),
            }
    }

    pub fn request_toggle(&mut self, index: usize) {
        if self.is_loading() {
            return;
        }

        let pending = match self.tab {
            StartupTab::Run => self.entries.get(index).map(|entry| PendingToggle {
                tab: StartupTab::Run,
                index,
                identifier: entry.title.clone(),
                title: entry.title.clone(),
                command: entry.command.clone(),
                enabling: !entry.enabled,
                manual_restore: false,
                system: false,
            }),
            StartupTab::Tasks => self.tasks.get(index).map(|entry| PendingToggle {
                tab: StartupTab::Tasks,
                index,
                identifier: entry.path.clone(),
                title: entry.title.clone(),
                command: entry.path.clone(),
                enabling: !entry.enabled,
                manual_restore: false,
                system: entry.system,
            }),
            StartupTab::Services => self.services.get(index).map(|entry| PendingToggle {
                tab: StartupTab::Services,
                index,
                identifier: entry.name.clone(),
                title: entry.title.clone(),
                command: entry.command.clone(),
                enabling: !entry.enabled,
                manual_restore: !entry.enabled && !entry.managed,
                system: entry.system,
            }),
        };
        self.pending = pending;
    }

    pub fn cancel_toggle(&mut self) {
        self.pending = None;
    }

    pub fn confirm_toggle(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };

        let index = match pending.tab {
            StartupTab::Run => self
                .entries
                .get(pending.index)
                .filter(|entry| entry.title == pending.title && entry.command == pending.command)
                .map(|_| pending.index),
            StartupTab::Tasks => self
                .tasks
                .iter()
                .position(|entry| entry.path == pending.identifier),
            StartupTab::Services => self
                .services
                .iter()
                .position(|entry| entry.name.eq_ignore_ascii_case(&pending.identifier)),
        };
        let Some(index) = index else {
            self.set_error("Запись изменилась; обновите список.".to_owned());
            return;
        };
        self.toggle_at(pending.tab, index);
    }

    fn toggle_at(&mut self, tab: StartupTab, index: usize) {
        if self.toggle_job.is_some() {
            return;
        }
        match tab {
            StartupTab::Run => self.toggle_run(index),
            StartupTab::Tasks => self.toggle_task(index),
            StartupTab::Services => self.toggle_service(index),
        }
    }

    fn toggle_run(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index).cloned() else {
            return;
        };
        self.toggle_job = Some(Job::spawn(
            "chipclean-startup-toggle",
            move |_cancel, tx| {
                let result = startup::toggle_entry(&entry).map_err(|err| err.to_string());
                let _ = tx.send(ToggleEvent::Done {
                    tab: StartupTab::Run,
                    result,
                });
            },
        ));
    }

    fn toggle_task(&mut self, index: usize) {
        let Some(entry) = self.tasks.get(index).cloned() else {
            return;
        };
        self.toggle_job = Some(Job::spawn(
            "chipclean-startup-toggle",
            move |_cancel, tx| {
                let result = tasks::set_task_enabled(&entry.path, !entry.enabled)
                    .map_err(|err| err.to_string());
                let _ = tx.send(ToggleEvent::Done {
                    tab: StartupTab::Tasks,
                    result,
                });
            },
        ));
    }

    fn toggle_service(&mut self, index: usize) {
        let Some(entry) = self.services.get(index).cloned() else {
            return;
        };
        self.toggle_job = Some(Job::spawn(
            "chipclean-startup-toggle",
            move |_cancel, tx| {
                let result = services::toggle_service(&entry).map_err(|err| err.to_string());
                let _ = tx.send(ToggleEvent::Done {
                    tab: StartupTab::Services,
                    result,
                });
            },
        ));
    }

    fn poll_toggle(&mut self) {
        let (events, disconnected) = job::drain(&self.toggle_job);
        let mut finished = false;
        let mut reload_tab = None;
        for event in events {
            finished = true;
            match event {
                ToggleEvent::Done { tab, result } => {
                    match result {
                        Ok(message) => self.set_ok(message),
                        Err(err) => self.set_error(err),
                    }
                    reload_tab = Some(tab);
                }
            }
        }
        if !finished && !disconnected {
            return;
        }
        if let Some(mut job) = self.toggle_job.take() {
            job.finish();
        }
        if !finished {
            self.set_error("Операция прервана: поток завершился неожиданно.".to_owned());
        }
        match reload_tab {
            Some(StartupTab::Run) => self.reload_run(),
            Some(StartupTab::Tasks) => self.reload_tasks(),
            Some(StartupTab::Services) => self.reload_services(),
            None => {}
        }
    }

    pub fn total_count(&self, tab: StartupTab) -> usize {
        match tab {
            StartupTab::Run => self.entries.len(),
            StartupTab::Tasks => self.tasks.len(),
            StartupTab::Services => self.services.len(),
        }
    }

    pub fn counts(&self, tab: StartupTab) -> (usize, usize) {
        match tab {
            StartupTab::Run => (
                self.entries.iter().filter(|entry| entry.enabled).count(),
                self.entries.iter().filter(|entry| !entry.enabled).count(),
            ),
            StartupTab::Tasks => (
                self.tasks.iter().filter(|entry| entry.enabled).count(),
                self.tasks.iter().filter(|entry| !entry.enabled).count(),
            ),
            StartupTab::Services => (
                self.services.iter().filter(|entry| entry.enabled).count(),
                self.services.iter().filter(|entry| !entry.enabled).count(),
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        registry::RegistryHive,
        startup::{StartupEntry, StartupEntryKind, StartupSource},
    };

    fn run_entry() -> StartupEntry {
        StartupEntry {
            title: "Test App".to_owned(),
            command: r"C:\Test\app.exe".to_owned(),
            source: StartupSource::HkcuRun,
            enabled: true,
            kind: StartupEntryKind::Registry {
                hive: RegistryHive::CurrentUser,
                key_path: r"Software\Microsoft\Windows\CurrentVersion\Run".to_owned(),
                value_name: "Test App".to_owned(),
            },
        }
    }

    #[test]
    fn run_toggle_requires_confirmation() {
        let mut state = StartupState::new();
        state.entries.push(run_entry());

        state.request_toggle(0);

        let pending = state.pending.as_ref().expect("pending toggle");
        assert_eq!(pending.tab, StartupTab::Run);
        assert!(!pending.enabling);
        assert!(state.message.is_none());
    }

    #[test]
    fn run_toggle_is_dropped_when_the_entry_changed() {
        let mut state = StartupState::new();
        state.entries.push(run_entry());
        state.request_toggle(0);
        state.entries[0].title = "Other".to_owned();

        state.confirm_toggle();

        assert!(state.pending.is_none());
        assert!(
            state
                .message
                .as_ref()
                .is_some_and(|message| message.is_error)
        );
    }
}
