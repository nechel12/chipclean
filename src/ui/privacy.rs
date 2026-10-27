use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    core::{
        privacy::{DataTarget, TargetScan},
        privacy_state::PrivacyState,
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut PrivacyState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Privacy);

    egui::ScrollArea::vertical()
        .id_salt("privacy-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            info_card(ui);
            ui.add_space(12.0);
            summary(ui, state);
            ui.add_space(12.0);

            let targets = state.targets.clone();
            let groups = unique_groups(&targets);
            for group in groups {
                group_header(ui, group, &targets, state);
                for target in targets.iter().filter(|target| target.group == group) {
                    let scan = state.scans.get(&target.id);
                    if target_row(ui, target, scan, state.is_selected(target)) {
                        state.toggle(&target.id);
                    }
                    ui.add_space(3.0);
                }
                ui.add_space(10.0);
            }
        });

    if state.pending_clean {
        confirmation_modal(ui.ctx(), state);
    }
}

fn info_card(ui: &mut Ui) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(10), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    ph::FINGERPRINT,
                    FontId::new(21.0, FontFamily::Proportional),
                    ACCENT,
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Чистка личных данных")
                            .font(font_semibold(14.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(
                            "История браузеров, списки файлов, недавние документы и кэш уведомлений. В Firefox удаляются только посещения — закладки сохраняются. Удаление личных данных необратимо; браузеры лучше закрыть.",
                        )
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                });
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "Приватность", ACCENT);
                    },
                );
            });
        });
}

fn summary(ui: &mut Ui, state: &mut PrivacyState) {
    let (files, bytes, history, registry) = state.selected_stats();
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
                            "Выбрано: {} · {}",
                            selected_label(state),
                            format::bytes(bytes)
                        ))
                        .font(font_semibold(14.0))
                        .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · посещений: {history} · записей реестра: {registry}",
                            format::files(files)
                        ))
                        .font(font(9.5))
                        .color(TEXT_MUTED),
                    );
                    if let Some(message) = &state.message {
                        ui.add_space(3.0);
                        ui.label(
                            RichText::new(&message.text)
                                .font(font(9.0))
                                .color(if message.is_error { ACCENT } else { SUCCESS }),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if state.is_scanning() || state.is_cleaning() {
                        if widgets::secondary_button(ui, "Отмена").clicked() {
                            if state.is_scanning() {
                                state.cancel_scan();
                            } else {
                                state.cancel_cleaning();
                            }
                        }
                    } else {
                        if widgets::danger_button(
                            ui,
                            "Очистить выбранное",
                            files > 0 || history > 0 || registry > 0,
                        )
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
                    }
                });
            });
            if let Some(progress) = state.scan_progress() {
                ui.add_space(10.0);
                widgets::progress(
                    ui,
                    progress.done as f32 / progress.total.max(1) as f32,
                    5.0,
                    ACCENT,
                );
                ui.add_space(6.0);
                let current = progress
                    .current
                    .as_deref()
                    .and_then(|id| state.targets.iter().find(|target| target.id == id))
                    .map(|target| target.title.as_str())
                    .unwrap_or("Подготовка…");
                ui.label(
                    RichText::new(format!(
                        "Проверка: {} из {} · {current}",
                        progress.done, progress.total
                    ))
                    .font(font(10.0))
                    .color(TEXT_MUTED),
                );
            } else if let Some(progress) = state.clean_progress() {
                ui.add_space(10.0);
                widgets::progress(
                    ui,
                    progress.done as f32 / progress.total.max(1) as f32,
                    5.0,
                    ACCENT,
                );
                ui.add_space(6.0);
                let current = progress
                    .current
                    .as_deref()
                    .and_then(|id| state.targets.iter().find(|target| target.id == id))
                    .map(|target| target.title.as_str())
                    .unwrap_or("Подготовка…");
                let label = if progress.rescan {
                    "Обновление"
                } else {
                    "Очистка"
                };
                ui.label(
                    RichText::new(format!(
                        "{label}: {} из {} · {current}",
                        progress.done, progress.total
                    ))
                    .font(font(10.0))
                    .color(TEXT_MUTED),
                );
            }
            ui.add_space(8.0);
            widgets::inline_hint(
                ui,
                ph::WARNING,
                "Удаление истории и куки необратимо. Активные браузеры могут воссоздать файлы.",
                TEXT_DIM,
            );
        });
}

fn group_header(ui: &mut Ui, group: &str, targets: &[DataTarget], state: &PrivacyState) {
    let selected = targets
        .iter()
        .filter(|target| target.group == group && state.is_selected(target))
        .count();
    let total = targets
        .iter()
        .filter(|target| target.group == group)
        .count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(group).font(font_semibold(13.0)).color(TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widgets::badge(ui, &format!("{selected}/{total}"), TEXT_DIM);
        });
    });
    ui.add_space(2.0);
}

fn target_row(ui: &mut Ui, target: &DataTarget, scan: Option<&TargetScan>, selected: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), SURFACE_2);
    }

    let checkbox =
        Rect::from_center_size(rect.left_center() + Vec2::new(18.0, 0.0), Vec2::splat(18.0));
    if selected {
        ui.painter()
            .rect_filled(checkbox, CornerRadius::same(5), ACCENT);
        ui.painter().text(
            checkbox.center(),
            Align2::CENTER_CENTER,
            ph::CHECK,
            FontId::new(11.0, FontFamily::Proportional),
            Color32::WHITE,
        );
    } else {
        ui.painter()
            .rect_filled(checkbox, CornerRadius::same(5), BORDER);
        ui.painter()
            .rect_filled(checkbox.shrink(1.0), CornerRadius::same(4), BG);
    }

    let color = group_color(&target.group);
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, 0.0),
        Align2::LEFT_CENTER,
        if target.warning.is_some() {
            ph::WARNING
        } else {
            ph::FINGERPRINT
        },
        FontId::new(17.0, FontFamily::Proportional),
        color,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(72.0, -9.0),
        Align2::LEFT_CENTER,
        &target.title,
        font_semibold(11.5),
        TEXT,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(72.0, 9.0),
        Align2::LEFT_CENTER,
        &target.summary,
        font(9.5),
        TEXT_DIM,
    );

    let right = match scan {
        Some(scan) => {
            let mut parts = Vec::new();
            if scan.bytes > 0 {
                parts.push(format::bytes(scan.bytes));
            } else if scan.files > 0 {
                parts.push(format::files(scan.files));
            }
            if scan.history > 0 {
                parts.push(format::visits(scan.history));
            }
            if scan.registry > 0 {
                parts.push(format!("записей: {}", scan.registry));
            }
            if parts.is_empty() {
                "—".to_owned()
            } else {
                parts.join(" · ")
            }
        }
        None => "—".to_owned(),
    };
    ui.painter().text(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        Align2::RIGHT_CENTER,
        right,
        font_semibold(10.0),
        TEXT_MUTED,
    );

    let response = response
        .on_hover_text(format!(
            "{}\n{}{}",
            target.title,
            target.summary,
            target
                .warning
                .as_ref()
                .map(|warning| format!("\n{warning}"))
                .unwrap_or_default()
        ))
        .on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            selected,
            format!("{} — {}", target.title, target.summary),
        )
    });
    widgets::focus_ring(ui, &response, rect, 8);
    response.clicked()
}

fn confirmation_modal(ctx: &egui::Context, state: &mut PrivacyState) {
    let (files, bytes, history, registry) = state.selected_stats();
    let selected: Vec<(DataTarget, Vec<String>, u64)> = state
        .selected_targets()
        .into_iter()
        .map(|target| {
            let (items, count) = state
                .scans
                .get(&target.id)
                .map(|scan| (scan.registry_items.clone(), scan.registry))
                .unwrap_or_default();
            (target, items, count)
        })
        .collect();
    let mut action = ModalAction::None;
    let modal = egui::Modal::new(egui::Id::new("privacy-confirm"))
        .frame(
            Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(13))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(560.0);
            dialog_header(ui);
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!(
                    "{} · {} · посещений: {history} · записей реестра: {registry}",
                    format::files(files),
                    format::bytes(bytes)
                ))
                .font(font(10.5))
                .color(TEXT_MUTED),
            );
            ui.add_space(8.0);
            Frame::new()
                .fill(BG)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("privacy-confirm-list")
                        .max_height(170.0)
                        .show(ui, |ui| {
                            for (target, items, count) in &selected {
                                ui.label(
                                    RichText::new(&target.title)
                                        .font(font(10.0))
                                        .color(TEXT_MUTED),
                                );
                                if let Some(warning) = &target.warning {
                                    ui.label(RichText::new(warning).font(font(8.0)).color(WARNING));
                                }
                                for item in items.iter().take(12) {
                                    ui.label(RichText::new(item).font(font(7.5)).color(TEXT_DIM));
                                }
                                if *count > 12 {
                                    ui.label(
                                        RichText::new(format!("… ещё {}", count - 12))
                                            .font(font(7.5))
                                            .color(TEXT_DIM),
                                    );
                                }
                            }
                        });
                });
            ui.add_space(10.0);
            widgets::confirmation_checkbox(
                ui,
                &mut state.confirm_ack,
                "Я понимаю, что личные данные будут удалены безвозвратно",
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::danger_button(ui, "Удалить личные данные", state.confirm_ack)
                        .clicked()
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

    if action == ModalAction::None && modal.should_close() {
        action = ModalAction::Cancel;
    }
    match action {
        ModalAction::None => {}
        ModalAction::Cancel => state.cancel_clean(),
        ModalAction::Confirm => state.confirm_clean(),
    }
}

fn dialog_header(ui: &mut Ui) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(11), ACCENT_SOFT);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            ph::WARNING,
            FontId::new(21.0, FontFamily::Proportional),
            ACCENT,
        );
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new("Удалить личные данные?")
                    .font(font_semibold(17.0))
                    .color(TEXT),
            );
            ui.label(
                RichText::new("История, куки и списки файлов будут удалены безвозвратно.")
                    .font(font(10.0))
                    .color(TEXT_MUTED),
            );
        });
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalAction {
    None,
    Cancel,
    Confirm,
}

fn selected_label(state: &PrivacyState) -> String {
    format!("{} из {}", state.selected.len(), state.targets.len())
}

fn unique_groups(targets: &[DataTarget]) -> Vec<&str> {
    let mut groups = Vec::new();
    for target in targets {
        if !groups.contains(&target.group.as_str()) {
            groups.push(target.group.as_str());
        }
    }
    groups
}

fn group_color(group: &str) -> Color32 {
    match group {
        "Браузеры" => WARNING,
        "Приложения" => SUCCESS,
        _ => ACCENT,
    }
}
