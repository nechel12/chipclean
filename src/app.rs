use crate::{
    config::{self, Settings},
    core::community_state::CommunityState,
    core::disk_analysis_state::DiskAnalysisState,
    core::privacy_state::PrivacyState,
    core::registry_editor_state::EditorState,
    core::registry_snapshots_state::SnapshotState,
    core::registry_state::RegistryState,
    core::rollback_state::RollbackState,
    core::startup_state::StartupState,
    core::state::CleanupState,
    core::traces_state::TracesState,
    core::tweaks_state::TweaksState,
    core::uninstall_state::UninstallState,
    i18n, theme, ui, win,
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
    Express,
    DiskAnalysis,
    Startup,
    Traces,
    Tweaks,
    Programs,
    Settings,
    Help,
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
            "express" => Page::Express,
            "disk_analysis" => Page::DiskAnalysis,
            "registry" => Page::Registry,
            "startup" => Page::Startup,
            "traces" => Page::Traces,
            "tweaks" => Page::Tweaks,
            "programs" => Page::Programs,
            "settings" => Page::Settings,
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
    pub disk_analysis: DiskAnalysisState,
    pub startup: StartupState,
    pub traces: TracesState,
    pub tweaks: TweaksState,
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
        i18n::set_lang_force(i18n::Lang::from_code(&settings.lang));
        let cleanup = CleanupState::new(&settings);
        let registry = RegistryState::new();
        let registry_editor = EditorState::new();
        let snapshots = SnapshotState::new();
        let rollback = RollbackState::new();
        let privacy = PrivacyState::new();
        let community = CommunityState::new();
        let disk_analysis = DiskAnalysisState::new();
        let startup = StartupState::new();
        let traces = TracesState::new();
        let tweaks = TweaksState::new();
        let programs = UninstallState::new();

        let mut app = Self {
            page: initial_page(),
            settings,
            cleanup,
            registry,
            registry_editor,
            snapshots,
            rollback,
            privacy,
            community,
            disk_analysis,
            startup,
            traces,
            tweaks,
            programs,
            disk: win::disk::system_drive_space().ok(),
            disk_refreshed: Instant::now(),
            elevated: win::elevation::is_elevated(),
            toast: None,
        };
        app.handle_preview_reg_arg();
        app
    }

    /// Обрабатывает запуск из контекстного меню Проводника
    /// (`chipclean.exe --preview-reg <файл>`).
    fn handle_preview_reg_arg(&mut self) {
        let args: Vec<String> = std::env::args().collect();
        let Some(position) = args.iter().position(|arg| arg == "--preview-reg") else {
            return;
        };
        let Some(path) = args.get(position + 1) else {
            return;
        };
        self.registry_editor
            .open_import_path(std::path::PathBuf::from(path));
        self.page = Page::RegistryEditor;
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
        self.registry_editor.poll_replace();
        self.registry_editor.poll_import();
        self.registry_editor.poll_mutation();
        self.snapshots.poll();
        self.rollback.poll();
        self.privacy.poll_scan();
        self.privacy.poll_clean();
        self.community.poll_scan();
        self.community.poll_clean();
        self.community.poll_update();
        self.disk_analysis.poll();
        self.startup.poll();
        self.traces.poll();
        self.traces.poll_mutation();
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
            || self.registry_editor.is_busy()
            || self.snapshots.is_busy()
            || self.rollback.is_busy()
            || self.privacy.is_scanning()
            || self.privacy.is_cleaning()
            || self.community.is_scanning()
            || self.community.is_cleaning()
            || self.startup.is_loading()
            || self.programs.is_loading()
            || self.traces.is_scanning()
            || self.traces.is_mutating()
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
                Page::Express => ui::express::show(
                    ui,
                    &mut self.cleanup,
                    &mut self.privacy,
                    &mut self.startup,
                    &mut self.tweaks,
                    self.settings.downloads_older_than_days,
                    &mut self.page,
                ),
                Page::DiskAnalysis => ui::disk_analysis::show(ui, &mut self.disk_analysis),
                Page::Startup => ui::startup::show(ui, &mut self.startup),
                Page::Traces => ui::traces::show(ui, &mut self.traces),
                Page::Tweaks => ui::tweaks::show(ui, &mut self.tweaks),
                Page::Programs => ui::uninstall::show(ui, &mut self.programs),
                Page::Settings => ui::settings::show(ui, &mut self.settings),
                Page::Help => ui::help::show(ui),
            });

        if self.settings != settings_before
            && let Err(err) = config::save(&self.settings)
        {
            tracing::warn!(%err, "failed to save settings");
            self.toast("Не удалось сохранить настройки");
        }

        // Синхронизация языка интерфейса.
        if self.settings.lang != settings_before.lang {
            i18n::set_lang_force(i18n::Lang::from_code(&self.settings.lang));
        }

        // Синхронизация фоновой очистки с Планировщиком заданий.
        let schedule_changed = self.settings.background_cleanup_enabled
            != settings_before.background_cleanup_enabled
            || self.settings.background_cleanup_hour != settings_before.background_cleanup_hour
            || self.settings.background_cleanup_minute != settings_before.background_cleanup_minute;
        if schedule_changed
            && let Err(err) = crate::core::schedule::apply_enabled(
                self.settings.background_cleanup_enabled,
                self.settings.background_cleanup_hour,
                self.settings.background_cleanup_minute,
            )
        {
            tracing::warn!(%err, "failed to sync background cleanup task");
            self.toast("Не удалось обновить задачу фоновой очистки");
        }

        let startup_check_changed = self.settings.startup_check_enabled
            != settings_before.startup_check_enabled;
        if startup_check_changed
            && let Err(err) = crate::core::schedule::apply_startup_check_enabled(
                self.settings.startup_check_enabled,
            )
        {
            tracing::warn!(%err, "failed to sync startup check task");
            self.toast("Не удалось обновить задачу проверки при входе");
        }

        self.draw_toast(ui.ctx());
    }
}
