use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        registry_editor::{
            self, EditorHive, REG_BINARY, REG_DWORD, REG_MULTI_SZ, REG_QWORD, join_path,
        },
        registry_editor_state::{EditorDialog, EditorState, ValueMode, value_types},
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.ensure_loaded();
    common::page_header(ui, Page::RegistryEditor);

    info_card(ui);
    ui.add_space(12.0);
    toolbar(ui, state);
    ui.add_space(12.0);

    let height = ui.available_height().max(300.0);
    ui.horizontal_top(|ui| {
        let left_width = 330.0_f32.min(ui.available_width() * 0.38);
        ui.allocate_ui_with_layout(
            Vec2::new(left_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| tree_card(ui, state, height),
        );
        ui.add_space(10.0);
        let right_width = ui.available_width();
        ui.allocate_ui_with_layout(
            Vec2::new(right_width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| values_card(ui, state, height),
        );
    });

    search_window(ui.ctx(), state);
    dialog_window(ui.ctx(), state);
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
                    RichText::new(ph::WARNING)
                        .font(FontId::new(18.0, FontFamily::Proportional))
                        .color(WARNING),
                );
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Редактор реестра")
                            .font(font_semibold(13.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(
                            "Изменения применяются сразу. Перед записью и удалением создаётся .reg-бэкап; для HKLM нужны права администратора.",
                        )
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                });
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "Для опытных", WARNING);
                    },
                );
            });
        });
}

fn toolbar(ui: &mut Ui, state: &mut EditorState) {
    let mut new_hive = None;
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("registry-editor-hive")
                    .selected_text(state.hive.label())
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        for hive in [
                            EditorHive::Hkcu,
                            EditorHive::Hklm,
                            EditorHive::Hkcr,
                            EditorHive::Hku,
                            EditorHive::Hkcc,
                        ] {
                            if ui
                                .selectable_label(state.hive == hive, hive.label())
                                .clicked()
                            {
                                new_hive = Some(hive);
                            }
                        }
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(registry_editor::full_path(state.hive, &state.selected))
                        .font(FontId::new(9.5, FontFamily::Monospace))
                        .color(TEXT_MUTED),
                );
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut state.search_query)
                        .hint_text("Поиск по разделам и значениям")
                        .desired_width(260.0),
                );
                let submit_search =
                    search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if widgets::secondary_button_icon(ui, "Найти", ph::MAGNIFYING_GLASS).clicked()
                    || submit_search
                {
                    state.search();
                }
                if state.is_searching() {
                    ui.add_space(6.0);
                    ui.spinner();
                    ui.label(RichText::new("Поиск…").font(font(9.0)).color(TEXT_MUTED));
                }
                ui.add_space(8.0);
                if widgets::secondary_button_icon(ui, "Обновить", ph::ARROW_CLOCKWISE).clicked()
                {
                    state.refresh();
                }
                if widgets::secondary_button(ui, "Открыть бэкапы").clicked()
                    && let Err(err) = config::open_dir(&config::backups_dir())
                {
                    tracing::warn!(%err, "failed to open backups");
                }
            });

            if let Some(message) = &state.message {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&message.text)
                        .font(font(9.0))
                        .color(if message.is_error { ACCENT } else { SUCCESS }),
                );
            }
        });

    if let Some(hive) = new_hive {
        state.set_hive(hive);
    }
}

fn tree_card(ui: &mut Ui, state: &mut EditorState, height: f32) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_height(height - 22.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Разделы")
                        .font(font_semibold(12.0))
                        .color(TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(state.hive.short())
                            .font(FontId::new(9.0, FontFamily::Monospace))
                            .color(TEXT_DIM),
                    );
                });
            });
            ui.add_space(6.0);
            widgets::line(ui);
            ui.add_space(4.0);

            egui::ScrollArea::both()
                .id_salt("registry-editor-tree")
                .auto_shrink([false, false])
                .max_height(height - 74.0)
                .show(ui, |ui| {
                    tree_node(ui, state, String::new(), state.hive.short().to_owned(), 0);
                });
        });
}

fn tree_node(ui: &mut Ui, state: &mut EditorState, path: String, name: String, depth: usize) {
    let expanded = state.is_expanded(&path);
    let has_children = state
        .children
        .get(&path)
        .map(|children| !children.is_empty())
        .unwrap_or(true);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(260.0), 25.0),
        Sense::click(),
    );
    let selected = state.selected == path;

    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), ACCENT_SOFT);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), SURFACE_2);
    }

    let left = rect.left() + 8.0 + depth as f32 * 13.0;
    let arrow_rect = Rect::from_center_size(egui::pos2(left, rect.center().y), Vec2::splat(16.0));
    let arrow_response = ui.interact(
        arrow_rect,
        ui.id().with(("registry-tree-arrow", &path)),
        Sense::click(),
    );
    if has_children {
        ui.painter().text(
            arrow_rect.center(),
            Align2::CENTER_CENTER,
            if expanded {
                ph::CARET_DOWN
            } else {
                ph::CARET_RIGHT
            },
            FontId::new(10.0, FontFamily::Proportional),
            TEXT_DIM,
        );
    }
    ui.painter().text(
        egui::pos2(left + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        ph::FOLDER_SIMPLE,
        FontId::new(13.0, FontFamily::Proportional),
        if selected { ACCENT } else { WARNING },
    );
    ui.painter().text(
        egui::pos2(left + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        shorten(&name, 32),
        font(10.5),
        if selected { TEXT } else { TEXT_MUTED },
    );

    if arrow_response.clicked() {
        if expanded {
            state.collapse(&path);
        } else {
            state.expand(path.clone());
        }
        return;
    }
    if response.clicked() {
        state.select(path.clone());
    }

    if expanded {
        let children = state.children.get(&path).cloned().unwrap_or_default();
        for child in children {
            tree_node(ui, state, join_path(&path, &child), child, depth + 1);
        }
    }
}

fn values_card(ui: &mut Ui, state: &mut EditorState, height: f32) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_min_height(height - 24.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(registry_editor::full_path(state.hive, &state.selected))
                            .font(FontId::new(10.0, FontFamily::Monospace))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            format::records(state.values.len() as u64),
                            state
                                .selected_value
                                .as_deref()
                                .map(registry_editor::display_value_name)
                                .unwrap_or("значение не выбрано")
                        ))
                        .font(font(9.0))
                        .color(TEXT_DIM),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if small_button(ui, "Удалить раздел", ph::TRASH, !state.selected.is_empty())
                        .clicked()
                    {
                        state.open_delete_key();
                    }
                    if small_button(ui, "Создать значение", ph::PLUS, true).clicked()
                    {
                        state.open_new_value();
                    }
                    if small_button(ui, "Создать раздел", ph::FOLDER_PLUS, true).clicked()
                    {
                        state.open_create_key();
                    }
                });
            });

            ui.add_space(7.0);
            widgets::line(ui);
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if small_button(
                    ui,
                    "Изменить",
                    ph::PENCIL_SIMPLE,
                    state.selected_value.is_some(),
                )
                .clicked()
                    && let Some(name) = state.selected_value.clone()
                {
                    state.open_edit_value(name);
                }
                if small_button(
                    ui,
                    "Удалить значение",
                    ph::TRASH,
                    state.selected_value.is_some(),
                )
                .clicked()
                    && let Some(name) = state.selected_value.clone()
                {
                    state.open_delete_value(name);
                }
            });

            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("registry-editor-values")
                .auto_shrink([false, false])
                .max_height(height - 155.0)
                .show(ui, |ui| {
                    if state.values.is_empty() {
                        ui.label(
                            RichText::new("В этом разделе нет значений.")
                                .font(font(10.0))
                                .color(TEXT_DIM),
                        );
                    }
                    let mut action = ValueAction::None;
                    for row in &state.values {
                        let response =
                            value_row(ui, row, state.selected_value.as_deref() == Some(&row.name));
                        if response.double_clicked() {
                            action = ValueAction::Edit(row.name.clone());
                        } else if response.clicked() {
                            action = ValueAction::Select(row.name.clone());
                        }
                    }
                    match action {
                        ValueAction::None => {}
                        ValueAction::Select(name) => state.select_value(name),
                        ValueAction::Edit(name) => state.open_edit_value(name),
                    }
                });
        });
}

enum ValueAction {
    None,
    Select(String),
    Edit(String),
}

fn value_row(ui: &mut Ui, row: &registry_editor::RegValueRow, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), ACCENT_SOFT);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), SURFACE_2);
    }

    ui.painter().text(
        rect.left_center() + Vec2::new(8.0, -7.0),
        Align2::LEFT_CENTER,
        shorten(
            if row.name.is_empty() {
                "(По умолчанию)"
            } else {
                &row.name
            },
            46,
        ),
        font(10.0),
        if selected { TEXT } else { TEXT_MUTED },
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(8.0, 7.0),
        Align2::LEFT_CENTER,
        format!(
            "{} · {} байт",
            registry_editor::type_label(row.ty),
            row.size
        ),
        FontId::new(8.5, FontFamily::Monospace),
        TEXT_DIM,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        shorten(&row.preview, 70),
        FontId::new(9.0, FontFamily::Monospace),
        TEXT_MUTED,
    );

    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn search_window(ctx: &egui::Context, state: &mut EditorState) {
    if !state.search_open {
        return;
    }
    let mut open = true;
    let mut selected = None;
    egui::Window::new("Результаты поиска")
        .id(egui::Id::new("registry-search-results"))
        .open(&mut open)
        .default_width(820.0)
        .max_height(520.0)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!("Найдено: {}", state.search_hits.len()))
                    .font(font(9.5))
                    .color(TEXT_MUTED),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("registry-search-list")
                .max_height(430.0)
                .show(ui, |ui| {
                    for hit in &state.search_hits {
                        let title = if let Some(value) = &hit.value_name {
                            format!(
                                "{}  ·  {}",
                                registry_editor::full_path(state.hive, &hit.path),
                                registry_editor::display_value_name(value)
                            )
                        } else {
                            registry_editor::full_path(state.hive, &hit.path)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(title)
                                        .font(FontId::new(9.0, FontFamily::Monospace)),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            selected = Some(hit.clone());
                        }
                        ui.label(RichText::new(&hit.detail).font(font(8.0)).color(TEXT_DIM));
                        ui.add_space(2.0);
                    }
                });
        });

    if let Some(hit) = selected {
        state.open_search_hit(hit);
    } else if !open {
        state.search_open = false;
    }
}

fn dialog_window(ctx: &egui::Context, state: &mut EditorState) {
    let Some(mut dialog) = state.dialog.take() else {
        return;
    };
    let mut action = DialogAction::None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("registry-editor-dialog-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            if response.clicked() {
                action = DialogAction::Cancel;
            }
        });

    egui::Window::new("registry-editor-dialog")
        .id(egui::Id::new("registry-editor-dialog-window"))
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
            ui.set_max_width(680.0);
            match &mut dialog {
                EditorDialog::CreateKey { name, error } => {
                    dialog_header(
                        ui,
                        ph::FOLDER_PLUS,
                        "Создать раздел",
                        "Новый подключ выбранного раздела.",
                    );
                    ui.add_space(12.0);
                    ui.label(RichText::new("Имя раздела").font(font(9.0)).color(TEXT_DIM));
                    ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    dialog_error(ui, error);
                    dialog_footer(ui, "Создать", &mut action);
                }
                EditorDialog::Value {
                    mode,
                    original_name,
                    name,
                    ty,
                    text,
                    error,
                } => {
                    let editing = *mode == ValueMode::Edit;
                    dialog_header(
                        ui,
                        ph::PENCIL_SIMPLE,
                        if editing {
                            "Изменить значение"
                        } else {
                            "Создать значение"
                        },
                        if editing {
                            "Имя значения фиксировано; изменение сразу применяется к реестру."
                        } else {
                            "Значение будет создано или перезаписано после проверки."
                        },
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Имя").font(font(9.0)).color(TEXT_DIM));
                            if editing {
                                ui.label(
                                    RichText::new(registry_editor::display_value_name(
                                        original_name,
                                    ))
                                    .font(FontId::new(10.0, FontFamily::Monospace))
                                    .color(TEXT_MUTED),
                                );
                            } else {
                                ui.add(
                                    egui::TextEdit::singleline(name)
                                        .hint_text("(По умолчанию)")
                                        .desired_width(280.0),
                                );
                            }
                        });
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Тип").font(font(9.0)).color(TEXT_DIM));
                            egui::ComboBox::from_id_salt("registry-editor-value-type")
                                .selected_text(registry_editor::type_label(*ty))
                                .show_ui(ui, |ui| {
                                    for (value_ty, label) in value_types() {
                                        ui.selectable_value(ty, value_ty, label);
                                    }
                                });
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Данные").font(font(9.0)).color(TEXT_DIM));
                    let hint = match *ty {
                        REG_DWORD => "Десятичное число или 0x...",
                        REG_QWORD => "Десятичное число или 0x...",
                        REG_BINARY => "Hex: 01 02 FF",
                        REG_MULTI_SZ => "По одному значению на строку",
                        _ => "",
                    };
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .hint_text(hint)
                            .desired_rows(8)
                            .desired_width(f32::INFINITY)
                            .font(FontId::new(10.0, FontFamily::Monospace)),
                    );
                    dialog_error(ui, error);
                    dialog_footer(ui, "Сохранить", &mut action);
                }
                EditorDialog::DeleteValue { name } => {
                    dialog_header(
                        ui,
                        ph::TRASH,
                        "Удалить значение?",
                        "Перед удалением раздела будет создан .reg-бэкап.",
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(registry_editor::display_value_name(name))
                            .font(FontId::new(11.0, FontFamily::Monospace))
                            .color(TEXT),
                    );
                    dialog_footer(ui, "Удалить значение", &mut action);
                }
                EditorDialog::DeleteKey { path } => {
                    dialog_header(
                        ui,
                        ph::TRASH,
                        "Удалить раздел?",
                        "Будет удалён раздел со всеми вложенными ключами. Создаётся .reg-бэкап.",
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(registry_editor::full_path(state.hive, path))
                            .font(FontId::new(10.0, FontFamily::Monospace))
                            .color(TEXT),
                    );
                    ui.add_space(8.0);
                    widgets::inline_hint(
                        ui,
                        ph::WARNING,
                        "Действие нельзя отменить из интерфейса; используйте .reg-бэкап.",
                        WARNING,
                    );
                    dialog_footer(ui, "Удалить раздел", &mut action);
                }
            }
        });

    if action == DialogAction::None
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        action = DialogAction::Cancel;
    }

    match action {
        DialogAction::Cancel => state.close_dialog(),
        DialogAction::Save => {
            state.dialog = Some(dialog);
            state.submit_dialog();
        }
        DialogAction::None => state.dialog = Some(dialog),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogAction {
    None,
    Cancel,
    Save,
}

fn dialog_header(ui: &mut Ui, icon: &str, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), ACCENT_SOFT);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::new(19.0, FontFamily::Proportional),
            ACCENT,
        );
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(font_semibold(16.0)).color(TEXT));
            ui.label(RichText::new(subtitle).font(font(10.0)).color(TEXT_MUTED));
        });
    });
}

fn dialog_error(ui: &mut Ui, error: &Option<String>) {
    if let Some(error) = error {
        ui.add_space(8.0);
        ui.label(RichText::new(error).font(font(9.5)).color(ACCENT));
    }
}

fn dialog_footer(ui: &mut Ui, label: &str, action: &mut DialogAction) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::primary_button(ui, label, ph::FLOPPY_DISK, true).clicked() {
                *action = DialogAction::Save;
            }
            ui.add_space(8.0);
            if widgets::secondary_button(ui, "Отмена").clicked() {
                *action = DialogAction::Cancel;
            }
        });
    });
}

fn small_button(ui: &mut Ui, label: &str, icon: &str, enabled: bool) -> egui::Response {
    let width = 150.0_f32.min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, 30.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if !enabled {
        SURFACE_2
    } else if response.hovered() {
        SURFACE_3
    } else {
        SURFACE_2
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{icon}  {label}"),
        font(9.5),
        if enabled { TEXT } else { TEXT_DIM },
    );
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
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
