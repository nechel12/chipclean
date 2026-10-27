use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui,
    Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        registry_snapshots::{SnapshotMeta, SnapshotRoot, SnapshotValue},
        registry_snapshots_state::SnapshotState,
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut SnapshotState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Snapshots);

    egui::ScrollArea::vertical()
        .id_salt("registry-snapshots-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            info_card(ui);
            ui.add_space(12.0);
            create_card(ui, state);
            ui.add_space(12.0);
            list_card(ui, state);
            if state.diff.is_some() {
                ui.add_space(12.0);
                diff_card(ui, state);
            }
        });

    if state.pending_delete.is_some() {
        delete_modal(ui.ctx(), state);
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
                ui.label(
                    RichText::new(ph::CAMERA)
                        .font(FontId::new(19.0, FontFamily::Proportional))
                        .color(ACCENT),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Снимки реестра")
                            .font(font_semibold(13.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(
                            "Снимок сохраняет разделы, значения и хеши данных. Сравнение покажет появившиеся, удалённые и изменённые записи; сами данные реестра не копируются.",
                        )
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                });
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if widgets::secondary_button(ui, "Открыть папку").clicked()
                            && let Err(err) = config::open_dir(&config::snapshots_dir())
                        {
                            tracing::warn!(%err, "failed to open snapshots directory");
                        }
                    },
                );
            });
        });
}

fn create_card(ui: &mut Ui, state: &mut SnapshotState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Новый снимок")
                    .font(font_semibold(13.0))
                    .color(TEXT),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.name)
                        .hint_text("Название снимка (необязательно)")
                        .desired_width(320.0),
                );
                ui.add_space(8.0);
                if widgets::primary_button(ui, "Создать снимок", ph::CAMERA, true).clicked()
                {
                    state.create();
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("Что включить")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
            let roots = state.roots.clone();
            for root in &roots {
                let mut checked = state.is_root_selected(root);
                if widgets::confirmation_checkbox(
                    ui,
                    &mut checked,
                    &format!("{} · {}", root.label, root.display()),
                ) {
                    state.toggle_root(root);
                }
            }
            ui.add_space(4.0);
            widgets::inline_hint(
                ui,
                ph::INFO,
                "Большие ветки могут создаваться несколько секунд. Лимит — 120 000 разделов.",
                TEXT_DIM,
            );
            if let Some(message) = &state.message {
                ui.add_space(5.0);
                ui.label(
                    RichText::new(&message.text)
                        .font(font(9.5))
                        .color(if message.is_error { ACCENT } else { SUCCESS }),
                );
            }
        });
}

fn list_card(ui: &mut Ui, state: &mut SnapshotState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Сохранённые снимки")
                        .font(font_semibold(13.0))
                        .color(TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::badge(ui, &state.snapshots.len().to_string(), TEXT_DIM);
                });
            });
            ui.add_space(8.0);

            if state.snapshots.is_empty() {
                ui.label(
                    RichText::new("Снимков пока нет. Создайте первый снимок перед изменениями.")
                        .font(font(10.0))
                        .color(TEXT_DIM),
                );
                return;
            }

            let mut select = None;
            let mut compare = false;
            let mut delete = None;
            let snapshots = state.snapshots.clone();
            for meta in &snapshots {
                let selected = state.selected.as_deref() == Some(&meta.id);
                let (clicked, delete_clicked) = snapshot_row(ui, state, meta, selected);
                if delete_clicked {
                    delete = Some(meta.clone());
                } else if clicked {
                    select = Some(meta.id.clone());
                    compare = true;
                }
                ui.add_space(5.0);
            }
            if let Some(meta) = delete {
                state.selected = Some(meta.id);
                state.request_delete();
            } else {
                if let Some(id) = select {
                    state.select(id);
                }
                if compare {
                    state.compare();
                }
            }
        });
}

fn snapshot_row(
    ui: &mut Ui,
    state: &SnapshotState,
    meta: &SnapshotMeta,
    selected: bool,
) -> (bool, bool) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 64.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), ACCENT_SOFT);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), SURFACE_2);
    }
    ui.painter().text(
        rect.left_center() + Vec2::new(12.0, -10.0),
        Align2::LEFT_CENTER,
        &meta.name,
        font_semibold(11.5),
        if selected { TEXT } else { TEXT_MUTED },
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(12.0, 9.0),
        Align2::LEFT_CENTER,
        format!(
            "{} · {} разделов · {} значений · {}",
            format_date(&meta.created_at),
            meta.keys,
            meta.values,
            format::bytes(state.snapshot_size(meta))
        ),
        font(9.0),
        TEXT_DIM,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 66.0, rect.center().y),
        Align2::RIGHT_CENTER,
        if meta.truncated {
            "обрезан"
        } else {
            "снимок"
        },
        font(9.0),
        if meta.truncated { WARNING } else { TEXT_DIM },
    );

    let delete_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 28.0, rect.center().y),
        Vec2::new(34.0, 24.0),
    );
    let delete_response = ui.interact(
        delete_rect,
        ui.id().with(("snapshot-delete", &meta.id)),
        Sense::click(),
    );
    ui.painter().text(
        delete_rect.center(),
        Align2::CENTER_CENTER,
        ph::TRASH,
        FontId::new(13.0, FontFamily::Proportional),
        if delete_response.hovered() {
            ACCENT
        } else {
            TEXT_DIM
        },
    );

    let clicked = response
        .on_hover_text(format!(
            "{}\n{}",
            meta.name,
            meta.roots
                .iter()
                .map(SnapshotRoot::display)
                .collect::<Vec<_>>()
                .join("\n")
        ))
        .clicked();
    (
        clicked && !delete_response.clicked(),
        delete_response.clicked(),
    )
}

fn diff_card(ui: &mut Ui, state: &mut SnapshotState) {
    let Some(diff) = state.diff.clone() else {
        return;
    };
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Изменения после снимка")
                        .font(font_semibold(13.0))
                        .color(TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::secondary_button(ui, "Закрыть").clicked() {
                        state.diff = None;
                    }
                });
            });
            if diff.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Реестр не изменился с момента снимка.")
                        .font(font(10.5))
                        .color(SUCCESS),
                );
                return;
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                diff_stat(ui, "Разделы +", diff.added_keys.len(), SUCCESS);
                diff_stat(ui, "Разделы −", diff.removed_keys.len(), ACCENT);
                diff_stat(ui, "Значения +", diff.added_values.len(), SUCCESS);
                diff_stat(ui, "Значения −", diff.removed_values.len(), ACCENT);
                diff_stat(ui, "Изменено", diff.changed_values.len(), WARNING);
            });
            if diff.truncated {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Снимок или сравнение обрезаны лимитом.")
                        .font(font(9.0))
                        .color(WARNING),
                );
            }

            ui.add_space(8.0);
            diff_section(ui, "Добавленные разделы", &diff.added_keys);
            diff_section(ui, "Удалённые разделы", &diff.removed_keys);
            diff_values_section(ui, "Добавленные значения", &diff.added_values);
            diff_values_section(ui, "Удалённые значения", &diff.removed_values);
            diff_values_section(ui, "Изменённые значения", &diff.changed_values);
        });
}

fn diff_stat(ui: &mut Ui, label: &str, count: usize, color: Color32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).font(font(9.0)).color(TEXT_DIM));
        ui.label(
            RichText::new(count.to_string())
                .font(font_semibold(15.0))
                .color(color),
        );
    });
    ui.add_space(18.0);
}

fn diff_section(ui: &mut Ui, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("{title} · {}", items.len()))
        .id_salt(title)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(format!("{title}-list"))
                .max_height(150.0)
                .show(ui, |ui| {
                    for item in items.iter().take(500) {
                        ui.label(
                            RichText::new(item)
                                .font(FontId::new(8.5, FontFamily::Monospace))
                                .color(TEXT_MUTED),
                        );
                    }
                });
        });
}

fn diff_values_section(
    ui: &mut Ui,
    title: &str,
    items: &[crate::core::registry_snapshots::ValueDiff],
) {
    if items.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("{title} · {}", items.len()))
        .id_salt(title)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(format!("{title}-list"))
                .max_height(180.0)
                .show(ui, |ui| {
                    for item in items.iter().take(400) {
                        ui.label(
                            RichText::new(format!(
                                "{}\\{}",
                                item.path,
                                if item.name.is_empty() {
                                    "(По умолчанию)"
                                } else {
                                    &item.name
                                }
                            ))
                            .font(FontId::new(8.5, FontFamily::Monospace))
                            .color(TEXT),
                        );
                        if let Some(before) = &item.before {
                            ui.label(
                                RichText::new(format!("было: {}", value_preview(before)))
                                    .font(font(8.0))
                                    .color(TEXT_DIM),
                            );
                        }
                        if let Some(after) = &item.after {
                            ui.label(
                                RichText::new(format!("стало: {}", value_preview(after)))
                                    .font(font(8.0))
                                    .color(SUCCESS),
                            );
                        }
                        ui.add_space(3.0);
                    }
                });
        });
}

fn value_preview(value: &SnapshotValue) -> String {
    format!(
        "{} · {}",
        crate::core::registry_editor::type_label(value.ty),
        value.preview
    )
}

fn format_date(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

fn delete_modal(ctx: &egui::Context, state: &mut SnapshotState) {
    let Some(meta) = state.pending_delete.clone() else {
        return;
    };
    let mut action = DeleteAction::None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("snapshot-delete-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            if response.clicked() {
                action = DeleteAction::Cancel;
            }
        });

    egui::Window::new("snapshot-delete")
        .id(egui::Id::new("snapshot-delete-window"))
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
            ui.set_min_width(460.0);
            ui.label(
                RichText::new("Удалить снимок?")
                    .font(font_semibold(16.0))
                    .color(TEXT),
            );
            ui.add_space(6.0);
            ui.label(RichText::new(&meta.name).font(font(11.0)).color(TEXT_MUTED));
            ui.add_space(10.0);
            widgets::inline_hint(
                ui,
                ph::WARNING,
                "Файл снимка будет удалён. Сравнение с ним станет недоступно.",
                WARNING,
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::danger_button(ui, "Удалить снимок", true).clicked() {
                        action = DeleteAction::Confirm;
                    }
                    ui.add_space(8.0);
                    if widgets::secondary_button(ui, "Отмена").clicked() {
                        action = DeleteAction::Cancel;
                    }
                });
            });
        });

    if action == DeleteAction::None
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        action = DeleteAction::Cancel;
    }
    match action {
        DeleteAction::None => {}
        DeleteAction::Cancel => state.cancel_delete(),
        DeleteAction::Confirm => state.confirm_delete(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeleteAction {
    None,
    Cancel,
    Confirm,
}
