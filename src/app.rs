use crate::{
    config::{self, Settings},
    core::community_state::CommunityState,
    core::privacy_state::PrivacyState,
    core::registry_editor_state::EditorState,
    core::registry_snapshots_state::SnapshotState,
    core::registry_state::RegistryState,
    core::rollback_state::RollbackState,
    core::startup_state::StartupState,
    core::state::CleanupState,
    core::traces_state::TracesState,
    core::uninstall_state::UninstallState,
    theme, ui, win,
};
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Cleanup,
    Registry,
    RegistryEditor,
    Snapshots,
    Rollback,
    Privacy,
    Community,
    Startup,
    Traces,
    Programs,
    Settings,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cleanup => "Очистка",
            Self::Registry => "Реестр",
            Self::RegistryEditor => "Редактор реестра",
            Self::Snapshots => "Снимки реестра",
            Self::Rollback => "Центр отката",
            Self::Privacy => "Личные данные",
            Self::Community => "Чистка сообществом",
            Self::Startup => "Автозагрузка",
            Self::Traces => "Следы",
            Self::Programs => "Программы",
            Self::Settings => "Настройки",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Cleanup => "Очистка системы",
            Self::Registry => "Реестр",
            Self::RegistryEditor => "Редактор реестра",
            Self::Snapshots => "Снимки реестра",
            Self::Rollback => "Центр отката",
            Self::Privacy => "Чистка личных данных",
            Self::Community => "Чистка сообществом",
            Self::Startup => "Автозагрузка",
            Self::Traces => "Следы программ",
            Self::Programs => "Программы",
            Self::Settings => "Настройки",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Cleanup => "Сканирование, предпросмотр и безопасное удаление с подтверждением.",
            Self::Registry => {
                "Безопасная проверка реестра и обязательный .reg-бэкап перед изменениями."
            }
            Self::RegistryEditor => "Просмотр, поиск и изменение реестра с .reg-бэкапами.",
            Self::Snapshots => "Создание снимков и сравнение изменений реестра.",
            Self::Rollback => {
                "Восстановление реестра, карантина следов и отключённой автозагрузки."
            }
            Self::Privacy => "История браузеров, списки файлов и недавние документы.",
            Self::Community => "Правила очистки из открытой базы winapp2.ini.",
            Self::Startup => "Run/RunOnce, папки «Автозагрузка», планировщик задач и службы.",
            Self::Traces => "Битые ярлыки и следы программ: карантин или удаление.",
            Self::Programs => "Установленные программы и запуск их штатных деинсталляторов.",
            Self::Settings => "Правила очистки, расположение журналов и сведения о сборке.",
        }
    }
}

fn initial_page() -> Page {
    #[cfg(debug_assertions)]
    if let Ok(page) = std::env::var("CHIPCLEAN_START_PAGE") {
        return match page.as_str() {
            "registry_editor" => Page::RegistryEditor,
            "snapshots" => Page::Snapshots,
            "rollback" => Page::Rollback,
            "privacy" => Page::Privacy,
            "community" => Page::Community,
            _ => Page::Cleanup,
        };
    }
    Page::Cleanup
}

pub struct Toast {
    text: String,
    created: Instant,
    duration: Duration,
}

pub struct ChipCleanApp {
    pub page: Page,
    pub settings: Settings,
    pub cleanup: CleanupState,
    pub registry: RegistryState,
    pub registry_editor: EditorState,
    pub snapshots: SnapshotState,
    pub rollback: RollbackState,
    pub privacy: PrivacyState,
    pub community: CommunityState,
    pub startup: StartupState,
    pub traces: TracesState,
    pub programs: UninstallState,
    pub disk: Option<win::disk::DiskSpace>,
    disk_refreshed: Instant,
    pub elevated: bool,
    toast: Option<Toast>,
}

const DISK_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

impl ChipCleanApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);

        let settings = config::load();
        let cleanup = CleanupState::new(&settings);
        let registry = RegistryState::new();
        let registry_editor = EditorState::new();
        let snapshots = SnapshotState::new();
        let rollback = RollbackState::new();
        let privacy = PrivacyState::new();
        let community = CommunityState::new();
        let startup = StartupState::new();
        let traces = TracesState::new();
        let programs = UninstallState::new();

        Self {
            page: initial_page(),
            settings,
            cleanup,
            registry,
            registry_editor,
            snapshots,
            rollback,
            privacy,
            community,
            startup,
            traces,
            programs,
            disk: win::disk::system_drive_space().ok(),
            disk_refreshed: Instant::now(),
            elevated: win::elevation::is_elevated(),
            toast: None,
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            created: Instant::now(),
            duration: Duration::from_secs(4),
        });
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let expired = self
            .toast
            .as_ref()
            .is_some_and(|toast| toast.created.elapsed() > toast.duration);
        if expired {
            self.toast = None;
            return;
        }

        let Some(toast) = &self.toast else {
            return;
        };

        egui::Area::new(egui::Id::new("chipclean-toast"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-22.0, -22.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::SURFACE_3)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(16, 11))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&toast.text)
                                .font(theme::font(12.0))
                                .color(theme::TEXT),
                        );
                    });
            });

        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

impl eframe::App for ChipCleanApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.cleanup.poll();
        self.cleanup.poll_clean();
        self.registry.poll_scan();
        self.registry.poll_clean();
        self.registry_editor.poll_search();
        self.snapshots.poll();
        self.rollback.poll();
        self.privacy.poll_scan();
        self.privacy.poll_clean();
        self.community.poll_scan();
        self.community.poll_clean();
        self.startup.poll();
        self.traces.poll();
        self.programs.poll();

        if let Some(program) = self.programs.take_traces_request() {
            self.traces.scan_after_uninstall(&program);
            self.page = Page::Traces;
        }

        if self.disk_refreshed.elapsed() >= DISK_REFRESH_INTERVAL {
            if let Ok(disk) = win::disk::system_drive_space() {
                self.disk = Some(disk);
            }
            self.disk_refreshed = Instant::now();
        }
        ctx.request_repaint_after(DISK_REFRESH_INTERVAL);

        if self.cleanup.is_scanning()
            || self.cleanup.is_cleaning()
            || self.registry.is_scanning()
            || self.registry.is_cleaning()
            || self.registry_editor.is_searching()
            || self.snapshots.is_busy()
            || self.rollback.is_busy()
            || self.privacy.is_scanning()
            || self.privacy.is_cleaning()
            || self.community.is_scanning()
            || self.community.is_cleaning()
            || self.startup.is_loading()
            || self.programs.is_loading()
            || self.traces.is_scanning()
        {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let settings_before = self.settings.clone();

        ui::sidebar::show(self, ui);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(26, 22)),
            )
            .show(ui, |ui| match self.page {
                Page::Cleanup => ui::cleanup::show(ui, &mut self.cleanup, &mut self.settings),
                Page::Registry => ui::registry::show(ui, &mut self.registry),
                Page::RegistryEditor => ui::registry_editor::show(ui, &mut self.registry_editor),
                Page::Snapshots => ui::snapshots::show(ui, &mut self.snapshots),
                Page::Rollback => ui::rollback::show(ui, &mut self.rollback),
                Page::Privacy => ui::privacy::show(ui, &mut self.privacy),
                Page::Community => ui::community::show(ui, &mut self.community),
                Page::Startup => ui::startup::show(ui, &mut self.startup),
                Page::Traces => ui::traces::show(ui, &mut self.traces),
                Page::Programs => ui::uninstall::show(ui, &mut self.programs),
                Page::Settings => ui::settings::show(ui, &mut self.settings),
            });

        if self.settings != settings_before
            && let Err(err) = config::save(&self.settings)
        {
            tracing::warn!(%err, "failed to save settings");
            self.toast("Не удалось сохранить настройки");
        }

        self.draw_toast(ui.ctx());
    }
}
