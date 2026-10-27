use crate::{
    config::{self, Settings},
    core::registry_state::RegistryState,
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
            Self::Startup => "Реестр Run/RunOnce и папки «Автозагрузка»: обратимое отключение.",
            Self::Traces => "Битые ярлыки и осиротевшие папки программ: удаление в карантин.",
            Self::Programs => "Установленные программы и запуск их штатных деинсталляторов.",
            Self::Settings => "Правила очистки, расположение журналов и сведения о сборке.",
        }
    }
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
    pub startup: StartupState,
    pub traces: TracesState,
    pub programs: UninstallState,
    pub disk: Option<win::disk::DiskSpace>,
    pub elevated: bool,
    toast: Option<Toast>,
}

impl ChipCleanApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);

        let settings = config::load();
        let cleanup = CleanupState::new(&settings);
        let registry = RegistryState::new();
        let startup = StartupState::new();
        let traces = TracesState::new();
        let programs = UninstallState::new();

        Self {
            page: Page::Cleanup,
            settings,
            cleanup,
            registry,
            startup,
            traces,
            programs,
            disk: win::disk::system_drive_space().ok(),
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
        self.traces.poll();

        if self.cleanup.is_scanning()
            || self.cleanup.is_cleaning()
            || self.registry.is_scanning()
            || self.registry.is_cleaning()
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
