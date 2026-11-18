use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;
use serde::Deserialize;

use crate::{
    app::Page,
    core::{
        privacy_state::PrivacyState, startup_state::StartupState, state::CleanupState,
        tweaks_state::TweaksState,
    },
    i18n::t,
    theme::*,
    ui::{common, format, widgets},
};

#[derive(Debug, Deserialize)]
struct LastBackground {
    ts: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    files: u64,
    #[serde(default)]
    errors: u64,
    #[serde(default)]
    cancelled: bool,
    #[serde(default)]
    elapsed_ms: u64,
    #[serde(default)]
    message: String,
}

fn load_last_background() -> Option<LastBackground> {
    let path = crate::config::background_result_path();
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn background_summary_card(ui: &mut Ui, summary: &LastBackground) {
    Frame::new()
        .fill(ACCENT_SOFT)
        .stroke(Stroke::new(1.0, ACCENT))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::CLOCK)
                        .font(FontId::new(17.0, FontFamily::Proportional))
                        .color(ACCENT),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    let headline = if summary.event == "background_failed" {
                        "Последняя фоновая очистка не удалась".to_owned()
                    } else if summary.cancelled {
                        "Последняя фоновая очистка была отменена".to_owned()
                    } else {
                        "Последняя фоновая очистка".to_owned()
                    };
                    ui.label(
                        RichText::new(headline)
                            .font(font_semibold(12.5))
                            .color(TEXT),
                    );
                    let detail = if summary.event == "background_failed" {
                        summary.message.clone()
                    } else {
                        format!(
                            "Удалено {} · {} файлов · ошибок: {} · {} мс · {}",
                            format::bytes(summary.bytes),
                            summary.files,
                            summary.errors,
                            summary.elapsed_ms,
                            summary.ts.replace('T', " ").trim_end_matches('Z'),
                        )
                    };
                    ui.label(RichText::new(detail).font(font(9.5)).color(TEXT_MUTED));
                });
            });
        });
}

pub fn show(
    ui: &mut Ui,
    cleanup: &mut CleanupState,
    privacy: &mut PrivacyState,
    startup: &mut StartupState,
    tweaks: &mut TweaksState,
    downloads_days: u32,
    page: &mut Page,
) {
    common::page_header(ui, Page::Express);

    let busy = cleanup.is_scanning() || cleanup.is_cleaning() || privacy.is_scanning();

    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(
                    RichText::new(ph::LIGHTNING)
                        .font(FontId::new(16.0, FontFamily::Proportional))
                        .color(if busy { TEXT_DIM } else { Color32::WHITE }),
                )
                .fill(if busy { SURFACE_2 } else { ACCENT })
                .min_size(Vec2::new(40.0, 40.0))
                .corner_radius(10),
            )
            .clicked()
        {
            run_all(cleanup, privacy, startup, tweaks, downloads_days);
        }

        ui.vertical(|ui| {
            ui.label(
                RichText::new(t("express_check"))
                    .font(font_semibold(14.0))
                    .color(TEXT),
            );
            ui.label(
                RichText::new("Одной кнопкой: скан системы, личные данные, автозагрузка и твики.")
                    .font(font(10.0))
                    .color(TEXT_MUTED),
            );
        });
    });

    ui.add_space(18.0);

    if let Some(summary) = load_last_background() {
        background_summary_card(ui, &summary);
        ui.add_space(10.0);
    }

    if card_cleanup(ui, cleanup) {
        *page = Page::Cleanup;
    }
    ui.add_space(10.0);
    if card_privacy(ui, privacy) {
        *page = Page::Privacy;
    }
    ui.add_space(10.0);
    if card_startup(ui, startup) {
        *page = Page::Startup;
    }
    ui.add_space(10.0);
    if card_tweaks(ui, tweaks) {
        *page = Page::Tweaks;
    }
}

fn run_all(
    cleanup: &mut CleanupState,
    privacy: &mut PrivacyState,
    startup: &mut StartupState,
    tweaks: &mut TweaksState,
    downloads_days: u32,
) {
    cleanup.start_scan(downloads_days);
    privacy.scan();
    startup.ensure_loaded();
    tweaks.scan();
}

fn card_cleanup(ui: &mut Ui, cleanup: &CleanupState) -> bool {
    let (bytes, files) = cleanup
        .results
        .values()
        .fold((0u64, 0u64), |(b, f), outcome| {
            (b + outcome.bytes, f + outcome.files)
        });
    let status = if cleanup.is_cleaning() {
        "Идёт очистка…"
    } else if cleanup.is_scanning() {
        "Идёт сканирование…"
    } else if cleanup.results.is_empty() {
        "Скан не запускался"
    } else {
        "Скан завершён"
    };

    summary_card(
        ui,
        ph::BROOM,
        "Очистка системы",
        &format!("Найдено категорий: {}", cleanup.results.len()),
        status,
        bytes,
        files,
    )
}

fn card_privacy(ui: &mut Ui, privacy: &PrivacyState) -> bool {
    let (bytes, files) = privacy.scans.values().fold((0u64, 0u64), |(b, f), scan| {
        (b + scan.bytes, f + scan.files)
    });
    let status = if privacy.is_scanning() {
        "Идёт сканирование…"
    } else if privacy.scans.is_empty() {
        "Скан не запускался"
    } else {
        "Скан завершён"
    };

    summary_card(
        ui,
        ph::FINGERPRINT,
        "Личные данные",
        &format!("Просканировано целей: {}", privacy.scans.len()),
        status,
        bytes,
        files,
    )
}

fn card_startup(ui: &mut Ui, startup: &StartupState) -> bool {
    let run = startup.entries.len();
    let tasks = startup.tasks.len();
    let services = startup.services.len();
    let total = run + tasks + services;
    let status = if !startup.loaded_run && !startup.loaded_tasks && !startup.loaded_services {
        "Загрузка…"
    } else {
        "Список загружен"
    };

    summary_card(
        ui,
        ph::ROCKET_LAUNCH,
        "Автозагрузка",
        &format!("Записей: {run} · Задач: {tasks} · Служб: {services} (всего {total})"),
        status,
        0,
        0,
    )
}

fn card_tweaks(ui: &mut Ui, tweaks: &TweaksState) -> bool {
    let active = tweaks
        .entries
        .iter()
        .filter(|entry| entry.current == Some(true))
        .count();
    let changed = tweaks.pending_count();
    let status = if !tweaks.scanned {
        "Скан не запускался"
    } else if changed > 0 {
        "Есть ожидающие изменения"
    } else {
        "Скан завершён"
    };

    summary_card(
        ui,
        ph::SLIDERS_HORIZONTAL,
        "Твики",
        &format!("Всего: {} · Включено: {active}", tweaks.entries.len()),
        status,
        0,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
fn summary_card(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    detail: &str,
    status: &str,
    bytes: u64,
    files: u64,
) -> bool {
    let mut open = false;
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(10), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icon,
                    FontId::new(20.0, FontFamily::Proportional),
                    ACCENT,
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).font(font_semibold(13.5)).color(TEXT));
                    ui.add_space(2.0);
                    ui.label(RichText::new(detail).font(font(10.0)).color(TEXT_MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::secondary_button(ui, "Открыть").clicked() {
                                open = true;
                            }
                            ui.add_space(8.0);
                            widgets::badge(ui, status, if bytes > 0 { SUCCESS } else { TEXT_DIM });
                        });
                        if bytes > 0 {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {} файлов",
                                            format::bytes(bytes),
                                            files
                                        ))
                                        .font(font(9.5))
                                        .color(TEXT_DIM),
                                    );
                                },
                            );
                        }
                    });
                });
            });
        });
    open
}
