use std::process::Command;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        community_state::CommunityState,
        privacy::{DataTarget, TargetScan},
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut CommunityState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Community);

    egui::ScrollArea::vertical()
        .id_salt("community-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            warning_card(ui);
            ui.add_space(12.0);
            database_card(ui, state);
            ui.add_space(12.0);
            if !state.targets.is_empty() {
                summary(ui, state);
                ui.add_space(12.0);
                sections(ui, state);
            }
        });

    if state.pending_clean {
        confirmation_modal(ui.ctx(), state);
    }
}

fn warning_card(ui: &mut Ui) {
    Frame::new()
        .fill(Color32::from_rgb(52, 37, 18))
        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(232, 163, 61, 90)))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::WARNING)
                        .font(FontId::new(18.0, FontFamily::Proportional))
                        .color(WARNING),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Чистка сообществом · winapp2.ini")
                            .font(font_semibold(13.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(
                            "Это открытая база правил сообщества. Разработчики ChipClean не проверяют её содержимое и не несут ответственности за удалённые данные. Используйте на свой риск и проверяйте выбранные секции.",
                        )
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                });
            });
        });
}

fn database_card(ui: &mut Ui, state: &mut CommunityState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("База winapp2.ini")
                        .font(font_semibold(13.0))
                        .color(TEXT),
                );
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if widgets::secondary_button(ui, "Страница базы").clicked() {
                            open_database_page();
                        }
                        if widgets::secondary_button(ui, "Открыть папку").clicked()
                            && let Err(err) = config::open_dir(&config::log_dir())
                        {
                            tracing::warn!(%err, "failed to open community folder");
                        }
                    },
                );
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.db_path)
                        .desired_width(520.0)
                        .font(FontId::new(9.0, FontFamily::Monospace)),
                );
                ui.add_space(8.0);
                if widgets::primary_button(ui, "Загрузить базу", ph::UPLOAD_SIMPLE, true).clicked()
                {
                    state.load();
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Поддерживаются FileKey, RegKey, Detect, Default, Warning и ExcludeKey. Продвинутые флаги REGEX и REMOVESELF игнорируются.",
                )
                .font(font(9.0))
                .color(TEXT_DIM),
            );
            if let Some(message) = &state.message {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&message.text)
                        .font(font(9.0))
                        .color(if message.is_error { ACCENT } else { SUCCESS }),
                );
            }
        });
}

fn summary(ui: &mut Ui, state: &mut CommunityState) {
    let (files, bytes, registry) = state.selected_stats();
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Загружено секций: {} · выбрано: {}",
                            state.targets.len(),
                            state.selected.len()
                        ))
                        .font(font_semibold(13.0))
                        .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · записей реестра: {registry}",
                            format::files(files),
                            format::bytes(bytes)
                        ))
                        .font(font(9.5))
                        .color(TEXT_MUTED),
                    );
                });
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if widgets::danger_button(ui, "Очистить выбранное", files > 0 || registry > 0)
                            .clicked()
                        {
                            state.request_clean();
                        }
                        ui.add_space(8.0);
                        if widgets::secondary_button_icon(ui, "Проверить", ph::MAGNIFYING_GLASS)
                            .clicked()
                        {
                            state.scan();
                        }
                    },
                );
            });
            ui.add_space(8.0);
            widgets::inline_hint(
                ui,
                ph::INFO,
                "Проверка выбранных секций может занять время; очистка необратима и не заменяет снимок реестра.",
                TEXT_DIM,
            );
        });
}

fn sections(ui: &mut Ui, state: &mut CommunityState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .hint_text("Фильтр по названию или группе")
                        .desired_width(360.0),
                );
                ui.add_space(8.0);
                if widgets::secondary_button(ui, "Выбрать рекомендуемые").clicked()
                {
                    state.select_defaults();
                }
                if widgets::secondary_button(ui, "Снять выбор").clicked() {
                    state.clear_selection();
                }
            });
            ui.add_space(8.0);

            let filter = state.filter.to_lowercase();
            let targets: Vec<DataTarget> = state
                .targets
                .iter()
                .filter(|target| {
                    filter.is_empty()
                        || target.title.to_lowercase().contains(&filter)
                        || target.group.to_lowercase().contains(&filter)
                })
                .cloned()
                .collect();
            let shown = targets.len().min(400);
            ui.label(
                RichText::new(format!("Показано: {shown} из {}", targets.len()))
                    .font(font(8.5))
                    .color(TEXT_DIM),
            );
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .id_salt("community-sections")
                .max_height(440.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut current_group = String::new();
                    for target in targets.iter().take(shown) {
                        if target.group != current_group {
                            ui.add_space(5.0);
                            ui.label(
                                RichText::new(&target.group)
                                    .font(font_semibold(11.0))
                                    .color(TEXT),
                            );
                            ui.add_space(2.0);
                            current_group = target.group.clone();
                        }
                        let scan = state.scans.get(&target.id);
                        if target_row(ui, target, scan, state.is_selected(target)) {
                            state.toggle(&target.id);
                        }
                    }
                });
        });
}

fn target_row(ui: &mut Ui, target: &DataTarget, scan: Option<&TargetScan>, selected: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), SURFACE_2);
    }

    let checkbox =
        Rect::from_center_size(rect.left_center() + Vec2::new(16.0, 0.0), Vec2::splat(17.0));
    if selected {
        ui.painter()
            .rect_filled(checkbox, CornerRadius::same(5), ACCENT);
        ui.painter().text(
            checkbox.center(),
            Align2::CENTER_CENTER,
            ph::CHECK,
            FontId::new(10.0, FontFamily::Proportional),
            Color32::WHITE,
        );
    } else {
        ui.painter()
            .rect_filled(checkbox, CornerRadius::same(5), BORDER);
        ui.painter()
            .rect_filled(checkbox.shrink(1.0), CornerRadius::same(4), BG);
    }

    ui.painter().text(
        rect.left_center() + Vec2::new(42.0, 0.0),
        Align2::LEFT_CENTER,
        if target.warning.is_some() {
            ph::WARNING
        } else {
            ph::BROOM
        },
        FontId::new(14.0, FontFamily::Proportional),
        if target.warning.is_some() {
            WARNING
        } else {
            TEXT_DIM
        },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 62.0, rect.center().y - 7.0),
        Align2::LEFT_CENTER,
        shorten(&target.title, 86),
        font(10.0),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 62.0, rect.center().y + 7.0),
        Align2::LEFT_CENTER,
        &target.summary,
        font(8.0),
        TEXT_DIM,
    );

    let size = scan
        .map(|scan| {
            if scan.bytes > 0 || scan.registry > 0 {
                format!(
                    "{}{}",
                    format::bytes(scan.bytes),
                    if scan.registry > 0 {
                        format!(" · {}", scan.registry)
                    } else {
                        String::new()
                    }
                )
            } else {
                "—".to_owned()
            }
        })
        .unwrap_or_else(|| "не проверено".to_owned());
    ui.painter().text(
        egui::pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        size,
        font(9.0),
        TEXT_DIM,
    );

    response
        .on_hover_text(format!(
            "{}\n{}{}",
            target.title,
            target.summary,
            target
                .warning
                .as_ref()
                .map(|warning| format!("\nВнимание: {warning}"))
                .unwrap_or_default()
        ))
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked()
}

fn confirmation_modal(ctx: &egui::Context, state: &mut CommunityState) {
    let (files, bytes, registry) = state.selected_stats();
    let selected = state.selected_targets();
    let mut action = ModalAction::None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("community-confirm-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            if response.clicked() {
                action = ModalAction::Cancel;
            }
        });

    egui::Window::new("community-confirm")
        .id(egui::Id::new("community-confirm-window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .frame(
            Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(13))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(600.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::WARNING)
                        .font(FontId::new(22.0, FontFamily::Proportional))
                        .color(ACCENT),
                );
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Очистка по базе сообщества")
                            .font(font_semibold(16.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(
                            "Правила не проверены разработчиками ChipClean. Очистка необратима.",
                        )
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                });
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!(
                    "{} · {} · записей реестра: {registry}",
                    format::files(files),
                    format::bytes(bytes)
                ))
                .font(font(10.0))
                .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            Frame::new()
                .fill(BG)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("community-confirm-list")
                        .max_height(160.0)
                        .show(ui, |ui| {
                            for target in selected.iter().take(30) {
                                ui.label(
                                    RichText::new(&target.title)
                                        .font(font(9.0))
                                        .color(TEXT_MUTED),
                                );
                            }
                        });
                });
            ui.add_space(10.0);
            widgets::confirmation_checkbox(
                ui,
                &mut state.confirm_ack,
                "Я понимаю, что база сообщества может удалить нужные данные",
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::danger_button(ui, "Очистить по базе", state.confirm_ack).clicked()
                    {
                        action = ModalAction::Confirm;
                    }
                    ui.add_space(8.0);
                    if widgets::secondary_button(ui, "Отмена").clicked() {
                        action = ModalAction::Cancel;
                    }
                });
            });
        });

    if action == ModalAction::None
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        action = ModalAction::Cancel;
    }
    match action {
        ModalAction::None => {}
        ModalAction::Cancel => state.cancel_clean(),
        ModalAction::Confirm => state.confirm_clean(),
    }
}

fn open_database_page() {
    let _ = Command::new("cmd")
        .args(["/C", "start", "", "https://github.com/MoscaDotTo/Winapp2"])
        .spawn();
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut shortened: String = text.chars().take(max).collect();
    shortened.push('…');
    shortened
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalAction {
    None,
    Cancel,
    Confirm,
}
