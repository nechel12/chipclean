use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        startup::{StartupEntry, StartupEntryKind, StartupSource},
        startup_state::StartupState,
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut StartupState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Startup);

    info_card(ui);
    ui.add_space(14.0);
    summary(ui, state);
    ui.add_space(14.0);
    list(ui, state);
}

fn info_card(ui: &mut Ui) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(11), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    ph::ROCKET_LAUNCH,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Автозагрузка Windows")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Run/RunOnce в HKCU и HKLM, а также папки «Автозагрузка». Отключение обратимо: реестр — .reg-бэкап, файлы — карантин. Планировщик задач — позже.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "Run + Startup", SUCCESS);
                    },
                );
            });
        });
}

fn summary(ui: &mut Ui, state: &mut StartupState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let total = state.entries.len() as u64;
                    ui.label(
                        RichText::new(if total == 0 {
                            "Автозагрузка пуста".to_owned()
                        } else {
                            format!("Найдено {}", format::records(total))
                        })
                        .font(font_semibold(15.0))
                        .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "Активно: {} · Отключено: {}",
                            state.enabled_count(),
                            state.disabled_count()
                        ))
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );

                    if let Some(message) = &state.message {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(&message.text)
                                .font(font(9.5))
                                .color(if message.is_error { ACCENT } else { SUCCESS }),
                        );
                    }
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if widgets::secondary_button_icon(
                            ui,
                            "Обновить",
                            ph::ARROW_CLOCKWISE,
                        )
                        .clicked()
                        {
                            state.refresh();
                        }
                        ui.add_space(8.0);
                        if widgets::secondary_button(ui, "Открыть бэкапы").clicked()
                            && let Err(err) = config::open_dir(&config::backups_dir())
                        {
                            tracing::warn!(%err, "failed to open backups directory");
                        }
                    },
                );
            });

            ui.add_space(10.0);
            widgets::inline_hint(
                ui,
                ph::SHIELD_CHECK,
                "Отключение можно отменить этим же переключателем. Для реестра создаётся .reg-бэкап.",
                TEXT_DIM,
            );
        });
}

fn list(ui: &mut Ui, state: &mut StartupState) {
    if state.entries.is_empty() {
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(ph::INFO)
                            .font(FontId::new(17.0, FontFamily::Proportional))
                            .color(TEXT_DIM),
                    );
                    ui.label(
                        RichText::new("Записи автозагрузки не найдены.")
                            .font(font(10.5))
                            .color(TEXT_MUTED),
                    );
                });
            });
        return;
    }

    let mut toggle_index = None;

    egui::ScrollArea::vertical()
        .id_salt("startup-entries")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, entry) in state.entries.iter().enumerate() {
                if startup_row(ui, entry, index) {
                    toggle_index = Some(index);
                }
                ui.add_space(4.0);
            }
        });

    if let Some(index) = toggle_index {
        state.toggle(index);
    }
}

fn startup_row(ui: &mut Ui, entry: &StartupEntry, index: usize) -> bool {
    let (rect, row_response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::hover());
    let hovered = row_response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), SURFACE_2);
    }

    let accent = if entry.enabled {
        source_color(entry.source)
    } else {
        TEXT_DIM
    };
    ui.painter().text(
        rect.left_center() + Vec2::new(20.0, 0.0),
        Align2::LEFT_CENTER,
        entry_icon(entry),
        FontId::new(19.0, FontFamily::Proportional),
        accent,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, -10.0),
        Align2::LEFT_CENTER,
        shorten(&entry.title, 40),
        font_semibold(12.0),
        if entry.enabled { TEXT } else { TEXT_MUTED },
    );
    let command = if entry.command.is_empty() {
        "—"
    } else {
        entry.command.as_str()
    };
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, 9.0),
        Align2::LEFT_CENTER,
        shorten(command, 70),
        FontId::new(9.0, FontFamily::Monospace),
        TEXT_DIM,
    );

    let switch_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 27.0, rect.center().y),
        Vec2::new(38.0, 21.0),
    );
    let switch_response = ui.interact(
        switch_rect,
        ui.id().with(("startup-toggle", index)),
        Sense::click(),
    );
    let t = ui.ctx().animate_bool(switch_response.id, entry.enabled);
    let background = if entry.enabled {
        ACCENT
    } else {
        Color32::from_rgb(72, 72, 82)
    };
    ui.painter()
        .rect_filled(switch_rect, CornerRadius::same(11), background);
    let knob_x = switch_rect.left() + 11.0 + (switch_rect.width() - 22.0) * t;
    ui.painter().circle_filled(
        egui::pos2(knob_x, switch_rect.center().y),
        7.5,
        Color32::WHITE,
    );

    ui.painter().text(
        egui::pos2(switch_rect.left() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        entry.source.short_label(),
        font(9.0),
        if entry.enabled { accent } else { TEXT_DIM },
    );

    let switch_response = switch_response
        .on_hover_text(if entry.enabled {
            "Отключить автозагрузку"
        } else {
            "Включить автозагрузку"
        })
        .on_hover_cursor(CursorIcon::PointingHand);

    let row_response = row_response.on_hover_text(&entry.command);
    let _ = row_response;

    switch_response.clicked()
}

fn entry_icon(entry: &StartupEntry) -> &'static str {
    if !entry.enabled {
        return ph::PROHIBIT;
    }

    match &entry.kind {
        StartupEntryKind::Registry { .. } => ph::ROCKET_LAUNCH,
        StartupEntryKind::Folder { .. } => ph::FOLDER_SIMPLE,
        StartupEntryKind::Disabled(_) => ph::PROHIBIT,
    }
}

fn source_color(source: StartupSource) -> Color32 {
    match source {
        StartupSource::HkcuRun | StartupSource::HklmRun => ACCENT,
        StartupSource::UserFolder | StartupSource::CommonFolder => WARNING,
    }
}

fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut shortened: String = text.chars().take(max_chars).collect();
    shortened.push('…');
    shortened
}
