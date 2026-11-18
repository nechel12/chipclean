use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    core::{
        tweaks::TweakKind,
        tweaks_state::{TweakEntry, TweaksState},
    },
    i18n::t,
    theme::*,
    ui::common,
};

pub fn show(ui: &mut Ui, state: &mut TweaksState) {
    if !state.scanned && !state.busy {
        state.scan();
    }

    common::page_header(ui, Page::Tweaks);

    toolbar(ui, state);

    if let Some(error) = &state.error {
        ui.add_space(10.0);
        error_banner(ui, error);
    }

    if let Some(summary) = &state.applied_summary {
        ui.add_space(10.0);
        summary_banner(ui, summary);
    }

    ui.add_space(12.0);

    egui::ScrollArea::vertical()
        .id_salt("tweaks-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let entries = state.entries.clone();
            for (index, entry) in entries.iter().enumerate() {
                match tweak_row(ui, entry, state) {
                    TweakRowAction::Toggle(enable) => {
                        state.toggle(index, !enable);
                    }
                    TweakRowAction::Reset => {
                        if let Err(err) = state.reset_one(index) {
                            state.error = Some(format!("Не удалось сбросить: {err}"));
                        }
                    }
                    TweakRowAction::Edit => {
                        state.begin_edit(index);
                    }
                    TweakRowAction::None => {}
                }
                if state.editing == Some(index) {
                    edit_panel(ui, state, index);
                }
                ui.add_space(4.0);
            }
        });
}

enum TweakRowAction {
    None,
    Toggle(bool),
    Reset,
    Edit,
}

fn toolbar(ui: &mut Ui, state: &mut TweaksState) {
    let pending = state.pending_count();

    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(
                    RichText::new(t("scan"))
                        .font(font_semibold(12.0))
                        .color(TEXT),
                )
                .min_size(Vec2::new(140.0, 34.0))
                .corner_radius(8),
            )
            .clicked()
        {
            state.scan();
        }

        if pending > 0 {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(t("discard"))
                            .font(font(12.0))
                            .color(TEXT_MUTED),
                    )
                    .min_size(Vec2::new(140.0, 34.0))
                    .corner_radius(8),
                )
                .clicked()
            {
                state.discard_changes();
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if pending > 0 {
                format!("{} ({pending})", t("apply"))
            } else {
                t("status").to_owned()
            };
            let enabled = pending > 0 && !state.busy;
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(
                        RichText::new(label)
                            .font(font_semibold(12.0))
                            .color(if enabled { Color32::WHITE } else { TEXT_DIM }),
                    )
                    .fill(if enabled { ACCENT } else { SURFACE_2 })
                    .min_size(Vec2::new(180.0, 34.0))
                    .corner_radius(8),
                )
                .clicked()
            {
                if let Err(err) = state.apply_pending() {
                    state.error = Some(format!("Не удалось применить изменения: {err}"));
                }
            }
        });
    });
}

fn tweak_row(ui: &mut Ui, entry: &TweakEntry, state: &TweaksState) -> TweakRowAction {
    let info = entry.info;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());

    let hovered = response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), SURFACE_2);
    }

    let is_fixed = matches!(info.kind, TweakKind::Fixed { .. });
    let checked = entry.pending.unwrap_or(entry.current.unwrap_or(false));
    let has_pending = entry.pending.is_some();

    // иконка статуса
    let icon_color = if has_pending {
        ACCENT
    } else if is_fixed && entry.current == Some(true) {
        SUCCESS
    } else {
        TEXT_DIM
    };
    let icon = if has_pending {
        ph::PENCIL_SIMPLE_LINE
    } else if is_fixed {
        if entry.current == Some(true) {
            ph::CHECK_CIRCLE
        } else {
            ph::CIRCLE
        }
    } else {
        ph::SLIDERS_HORIZONTAL
    };

    ui.painter().text(
        rect.left_center() + Vec2::new(18.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        FontId::new(18.0, FontFamily::Proportional),
        icon_color,
    );

    let rest_x = rect.left() + 46.0;

    ui.painter().text(
        egui::pos2(rest_x, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        info.title,
        font_semibold(12.5),
        TEXT,
    );

    let subtitle = build_subtitle(entry);
    ui.painter().text(
        egui::pos2(rest_x, rect.top() + 34.0),
        Align2::LEFT_CENTER,
        subtitle,
        font(10.0),
        TEXT_DIM,
    );

    // правая часть: чекбокс для фиксированных; действия для плавающих
    if is_fixed {
        draw_checkbox(ui, rect, checked, has_pending);
        if response.clicked() {
            return TweakRowAction::Toggle(checked);
        }
    } else {
        if state.editing == Some(0) || state.editing.is_some() {
            // не перехватываем клики строки во время редактирования
            let _ = rect;
            if response.clicked() {
                return TweakRowAction::Edit;
            }
        } else if ui
            .put(
                egui::Rect::from_min_max(
                    egui::pos2(rect.right() - 165.0, rect.top() + 10.0),
                    egui::pos2(rect.right() - 8.0, rect.bottom() - 10.0),
                ),
                egui::Button::new(
                    RichText::new("Изменить значение")
                        .font(font(10.5))
                        .color(ACCENT),
                )
                .fill(SURFACE_2)
                .corner_radius(8),
            )
            .clicked()
        {
            return TweakRowAction::Edit;
        } else if ui
            .put(
                egui::Rect::from_min_max(
                    egui::pos2(rect.right() - 90.0, rect.top() + 10.0),
                    egui::pos2(rect.right() - 8.0, rect.bottom() - 10.0),
                ),
                egui::Button::new(
                    RichText::new("Восстановить")
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                )
                .fill(SURFACE_2)
                .corner_radius(8),
            )
            .clicked()
        {
            return TweakRowAction::Reset;
        }
    }

    response.on_hover_cursor(CursorIcon::PointingHand);
    TweakRowAction::None
}

fn edit_panel(ui: &mut Ui, state: &mut TweaksState, index: usize) {
    let Some(entry) = state.entries.get(index) else {
        state.editing = None;
        return;
    };
    let info = entry.info;
    let unit = match info.kind {
        TweakKind::Floating { unit, .. } => unit,
        _ => "",
    };

    Frame::new()
        .fill(SURFACE_2)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Значение").font(font(10.5)).color(TEXT_MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut state.edit_buffer)
                        .desired_width(180.0)
                        .font(FontId::new(11.0, FontFamily::Monospace)),
                );
                if !unit.is_empty() {
                    ui.label(RichText::new(unit).font(font(10.5)).color(TEXT_DIM));
                }
                ui.add_space(8.0);
                let apply = ui
                    .add(
                        egui::Button::new(
                            RichText::new("Применить")
                                .font(font_semibold(11.0))
                                .color(Color32::WHITE),
                        )
                        .fill(ACCENT)
                        .corner_radius(7),
                    )
                    .clicked();
                let cancel = ui
                    .add(
                        egui::Button::new(
                            RichText::new("Отмена")
                                .font(font(11.0))
                                .color(TEXT_MUTED),
                        )
                        .corner_radius(7),
                    )
                    .clicked();

                if apply {
                    if let Err(err) = state.apply_edit(index) {
                        state.error = Some(format!("Не удалось применить значение: {err}"));
                    }
                } else if cancel {
                    state.cancel_edit();
                }
            });
        });
}

fn build_subtitle(entry: &TweakEntry) -> String {
    let info = entry.info;
    let path = crate::core::tweaks::full_path(info);
    match info.kind {
        TweakKind::Fixed { .. } => path,
        TweakKind::Floating { unit, min, max, .. } => {
            let value = entry.current_raw.clone().unwrap_or_else(|| "—".to_owned());
            let range = if !min.is_empty() && !max.is_empty() {
                format!(" · диапазон {min}–{max} {unit}")
            } else {
                String::new()
            };
            format!("{path} · текущее: {value} {unit}{range}")
        }
    }
}

fn draw_checkbox(ui: &Ui, rect: Rect, checked: bool, has_pending: bool) {
    let center = egui::pos2(rect.right() - 30.0, rect.center().y);
    let radius = 9.0;
    let fill = if checked || has_pending {
        ACCENT
    } else {
        SURFACE_3
    };
    ui.painter().circle_filled(center, radius, fill);
    if checked {
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            ph::CHECK,
            FontId::new(13.0, FontFamily::Proportional),
            Color32::WHITE,
        );
    }
}

fn error_banner(ui: &mut Ui, error: &str) {
    Frame::new()
        .fill(ERROR_SOFT)
        .stroke(Stroke::new(1.0, ERROR))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.label(
                RichText::new(error)
                    .font(font(11.5))
                    .color(ERROR_TEXT),
            );
        });
}

fn summary_banner(ui: &mut Ui, summary: &str) {
    Frame::new()
        .fill(ACCENT_SOFT)
        .stroke(Stroke::new(1.0, ACCENT))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.label(
                RichText::new(summary)
                    .font(font(11.5))
                    .color(TEXT),
            );
        });
}
