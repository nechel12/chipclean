#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod core;
mod theme;
mod ui;
mod win;

use anyhow::{Context, Result};
use single_instance::SingleInstance;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    let instance = match SingleInstance::new("ru.chipclean.single-instance") {
        Ok(instance) => instance,
        Err(err) => {
            win::dialog::error(
                "ChipClean",
                &format!("Не удалось инициализировать проверку единственного экземпляра:\n{err}"),
            );
            return Ok(());
        }
    };

    if !instance.is_single() {
        return Ok(());
    }

    let _log_guard = init_logging()?;
    let elevated = win::elevation::is_elevated();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        elevated,
        "ChipClean starting"
    );

    if !elevated && !dev_elevation_bypass() {
        drop(instance);
        if let Err(err) = win::elevation::relaunch_elevated() {
            tracing::warn!(%err, "elevation failed");
            win::dialog::error(
                "ChipClean",
                "Для работы программы нужны права администратора.\nЗапрос UAC был отменён или завершился ошибкой.",
            );
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ChipClean")
            .with_app_id("ru.chipclean.app")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([1000.0, 660.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "ChipClean",
        options,
        Box::new(|cc| Ok(Box::new(app::ChipCleanApp::new(cc)))),
    );

    if let Err(err) = result {
        tracing::error!(%err, "eframe failed");
        win::dialog::error(
            "ChipClean",
            &format!("Не удалось запустить интерфейс:\n{err}"),
        );
    }

    Ok(())
}

fn dev_elevation_bypass() -> bool {
    cfg!(debug_assertions) && std::env::var_os("CHIPCLEAN_SKIP_ELEVATION").is_some()
}

fn init_logging() -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let dir = config::log_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("не удалось создать каталог логов {}", dir.display()))?;
    let appender = tracing_appender::rolling::daily(&dir, "chipclean.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("chipclean=info,warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();
    Ok(guard)
}
