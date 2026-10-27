use egui::{CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Stroke, Ui};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config::{self, Settings},
    theme::*,
    ui::{common, widgets},
};

pub fn show(ui: &mut Ui, settings: &mut Settings) {
    common::page_header(ui, Page::Settings);

    ui.columns(2, |columns| {
        card(
            &mut columns[0],
            "Правила «Загрузок»",
            |ui| {
                ui.label(
                    RichText::new("Удалять только файлы старше")
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                );
                ui.add_space(6.0);
                ui.add(
                    egui::Slider::new(&mut settings.downloads_older_than_days, 1..=365)
                        .suffix(" дн."),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Файлы моложе выбранного срока не попадают в скан и не могут быть удалены.",
                    )
                    .font(font(9.5))
                    .color(TEXT_DIM),
                );
            },
        );

        card(
            &mut columns[0],
            "Гарантии безопасности",
            |ui| {
                rule_row(ui, "Удаление: только после предпросмотра");
                rule_row(ui, "Загрузки: отдельный шаг подтверждения");
                rule_row(ui, "Корзина и карантин: отдельный шаг");
                rule_row(ui, "Задачи и службы: подтверждение перед изменением");
                rule_row(ui, "Все операции пишутся в журнал");
            },
        );

        card(&mut columns[1], "Данные приложения", |ui| {
            ui.label(RichText::new("Настройки").font(font(9.5)).color(TEXT_DIM));
            ui.label(
                RichText::new(config::settings_path().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            ui.label(RichText::new("Журналы").font(font(9.5)).color(TEXT_DIM));
            ui.label(
                RichText::new(config::log_dir().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Журнал операций")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
            ui.label(
                RichText::new(config::journal_path().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Бэкапы реестра")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
            ui.label(
                RichText::new(config::backups_dir().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Состояние служб")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
            ui.label(
                RichText::new(config::disabled_services_path().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Карантин автозагрузки")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
            ui.label(
                RichText::new(config::startup_quarantine_dir().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Карантин следов")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
            ui.label(
                RichText::new(config::traces_quarantine_dir().display().to_string())
                    .font(font(9.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(10.0);

            if widgets::secondary_button(ui, "Открыть настройки").clicked() {
                open_dir(&config::config_dir());
            }
            ui.add_space(6.0);
            if widgets::secondary_button(ui, "Открыть логи").clicked() {
                open_dir(&config::log_dir());
            }
            ui.add_space(6.0);
            if widgets::secondary_button(ui, "Открыть бэкапы").clicked() {
                open_dir(&config::backups_dir());
            }
            ui.add_space(6.0);
            if widgets::secondary_button(ui, "Карантин автозагрузки").clicked()
            {
                open_dir(&config::startup_quarantine_dir());
            }
            ui.add_space(6.0);
            if widgets::secondary_button(ui, "Карантин следов").clicked() {
                open_dir(&config::traces_quarantine_dir());
            }
        });

        card(&mut columns[1], "О программе", |ui| {
            ui.label(
                RichText::new("ChipClean 0.1.0")
                    .font(font_semibold(14.0))
                    .color(TEXT),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new("Сканер и безопасная очистка")
                    .font(font(10.0))
                    .color(TEXT_MUTED),
            );
            ui.add_space(12.0);
            about_line(ui, "Rust 2024 · eframe/egui");
            about_line(ui, "Windows 10/11 x64 · UAC");
            about_line(ui, "Репозиторий приватный");
        });
    });
}

fn card(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.label(RichText::new(title).font(font_semibold(12.5)).color(TEXT));
            ui.add_space(12.0);
            add_contents(ui);
        });
    ui.add_space(12.0);
}

fn rule_row(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(ph::CHECK_CIRCLE)
                .font(FontId::new(12.0, FontFamily::Proportional))
                .color(SUCCESS),
        );
        ui.label(RichText::new(text).font(font(9.5)).color(TEXT_MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widgets::badge(ui, "Всегда", SUCCESS);
        });
    });
    ui.add_space(4.0);
}

fn about_line(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(ph::DOT)
                .font(FontId::new(12.0, FontFamily::Proportional))
                .color(ACCENT),
        );
        ui.label(RichText::new(text).font(font(9.5)).color(TEXT_MUTED));
    });
}

fn open_dir(path: &std::path::Path) {
    if let Err(err) = config::open_dir(path) {
        tracing::warn!(%err, path = %path.display(), "failed to open directory");
    }
}
