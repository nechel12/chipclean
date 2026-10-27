use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        uninstall::ProgramEntry,
        uninstall_state::{ProgramSort, UninstallState},
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut UninstallState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Programs);

    info_card(ui);
    ui.add_space(14.0);
    summary(ui, state);
    if state.last_launched.is_some() {
        ui.add_space(10.0);
        follow_up_card(ui, state);
    }
    ui.add_space(10.0);
    filter_bar(ui, state);
    ui.add_space(8.0);
    list(ui, state);

    if state.pending_uninstall.is_some() {
        confirmation_modal(ui.ctx(), state);
    }
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
                    ph::PACKAGE,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Штатные деинсталляторы")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "ChipClean не удаляет программы сам: он запускает официальный деинсталлятор выбранной программы. Store/MSIX-приложения, не попавшие в Uninstall, здесь не отображаются.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "Uninstall", SUCCESS);
                    },
                );
            });
        });
}

fn summary(ui: &mut Ui, state: &mut UninstallState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let total = state.entries.len();
                    let shown = state.filtered_indices().len();
                    ui.label(
                        RichText::new(format!("Установлено {total} программ"))
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "Показано {shown} · после деинсталляции нажмите «Обновить», затем проверьте следы"
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

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::secondary_button_icon(ui, "Обновить", ph::ARROW_CLOCKWISE).clicked()
                    {
                        state.refresh();
                    }
                    if state.is_loading() {
                        ui.add_space(6.0);
                        ui.spinner();
                    }
                });
            });
        });
}

fn follow_up_card(ui: &mut Ui, state: &mut UninstallState) {
    let Some(program) = state.last_launched.clone() else {
        return;
    };

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, ACCENT))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(10), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    ph::FOOTPRINTS,
                    FontId::new(19.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Проверить следы после удаления?")
                            .font(font_semibold(12.5))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "Когда деинсталлятор «{program}» завершит работу, найдите оставшиеся папки и ярлыки в разделе «Следы» и отправьте их в карантин."
                        ))
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, "Найти следы", ph::MAGNIFYING_GLASS, true)
                        .clicked()
                    {
                        state.request_traces_scan();
                    }
                });
            });
        });
}

fn filter_bar(ui: &mut Ui, state: &mut UninstallState) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [320.0, 34.0],
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Поиск по названию или издателю"),
        );

        ui.add_space(8.0);
        egui::ComboBox::from_id_salt("programs-sort")
            .selected_text(state.sort.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.sort, ProgramSort::Name, "По имени");
                ui.selectable_value(&mut state.sort, ProgramSort::Publisher, "По издателю");
                ui.selectable_value(&mut state.sort, ProgramSort::Size, "По размеру");
            });
    });
}

fn list(ui: &mut Ui, state: &mut UninstallState) {
    let indices = state.filtered_indices();

    if state.entries.is_empty() {
        empty_card(ui, "Программы не найдены.");
        return;
    }
    if indices.is_empty() {
        empty_card(ui, "По запросу ничего не найдено.");
        return;
    }

    let mut clicked = None;

    egui::ScrollArea::vertical()
        .id_salt("programs-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for index in indices {
                let entry = &state.entries[index];
                if program_row(ui, entry, index) {
                    clicked = Some(index);
                }
                ui.add_space(6.0);
            }
        });

    if let Some(index) = clicked {
        state.request_uninstall(index);
    }
}

fn program_row(ui: &mut Ui, entry: &ProgramEntry, index: usize) -> bool {
    let (rect, row_response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::hover());

    let fill = if row_response.hovered() {
        SURFACE_2
    } else {
        SURFACE
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);

    ui.painter().text(
        rect.left_center() + Vec2::new(20.0, 0.0),
        Align2::LEFT_CENTER,
        ph::PACKAGE,
        FontId::new(20.0, FontFamily::Proportional),
        if entry.command.is_some() {
            ACCENT
        } else {
            TEXT_DIM
        },
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, -14.0),
        Align2::LEFT_CENTER,
        shorten(&entry.display_name, 54),
        font_semibold(12.5),
        TEXT,
    );

    let mut details = Vec::new();
    if !entry.version.trim().is_empty() {
        details.push(format!("v{}", entry.version.trim()));
    }
    details.push(entry.publisher.clone());
    if entry.size_kb > 0 {
        details.push(format::bytes(u64::from(entry.size_kb) * 1024));
    }
    details.push(entry.source.label().to_owned());

    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, 6.0),
        Align2::LEFT_CENTER,
        shorten(&details.join(" · "), 90),
        font(9.5),
        TEXT_MUTED,
    );

    if let Some(location) = entry.install_location.as_ref().filter(|path| path.is_dir()) {
        let folder_rect = Rect::from_center_size(
            egui::pos2(rect.right() - 140.0, rect.center().y),
            Vec2::splat(32.0),
        );
        let folder_response = ui.interact(
            folder_rect,
            ui.id().with(("program-folder", index)),
            Sense::click(),
        );
        ui.painter().rect_filled(
            folder_rect,
            CornerRadius::same(8),
            if folder_response.hovered() {
                ACCENT_SOFT
            } else {
                SURFACE_3
            },
        );
        ui.painter().text(
            folder_rect.center(),
            Align2::CENTER_CENTER,
            ph::FOLDER_OPEN,
            FontId::new(15.0, FontFamily::Proportional),
            TEXT_MUTED,
        );
        let folder_response = folder_response
            .on_hover_text("Открыть папку установки")
            .on_hover_cursor(CursorIcon::PointingHand);
        folder_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Открыть папку установки {}", entry.display_name),
            )
        });
        widgets::focus_ring(ui, &folder_response, folder_rect, 8);
        if folder_response.clicked()
            && let Err(err) = config::open_dir(location)
        {
            tracing::warn!(%err, path = %location.display(), "failed to open install location");
        }
    }

    let button_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 64.0, rect.center().y),
        Vec2::new(104.0, 32.0),
    );
    let enabled = entry.command.is_some();
    let button_response = ui.interact(
        button_rect,
        ui.id().with(("program-uninstall", index)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let button_fill = if enabled && button_response.hovered() {
        ACCENT_SOFT
    } else {
        SURFACE_3
    };
    ui.painter()
        .rect_filled(button_rect, CornerRadius::same(8), button_fill);
    ui.painter().text(
        button_rect.center(),
        Align2::CENTER_CENTER,
        if enabled {
            "Удалить"
        } else {
            "Недоступно"
        },
        font(10.0),
        if enabled { ACCENT } else { TEXT_DIM },
    );

    let button_response = if enabled {
        button_response
            .on_hover_text(format!(
                "Запустить штатный деинсталлятор\n{}",
                entry.command_display
            ))
            .on_hover_cursor(CursorIcon::PointingHand)
    } else {
        button_response.on_hover_text("Штатный деинсталлятор не найден")
    };

    button_response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            enabled,
            format!("Удалить {}", entry.display_name),
        )
    });
    if enabled {
        widgets::focus_ring(ui, &button_response, button_rect, 8);
    }

    let row_response = row_response.on_hover_text(&entry.key_display);
    let _ = row_response;

    button_response.clicked() && enabled
}

fn empty_card(ui: &mut Ui, text: &str) {
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
                ui.label(RichText::new(text).font(font(10.5)).color(TEXT_MUTED));
            });
        });
}

fn confirmation_modal(ctx: &egui::Context, state: &mut UninstallState) {
    let Some(index) = state.pending_uninstall else {
        return;
    };
    let Some(entry) = state.entries.get(index).cloned() else {
        state.cancel_uninstall();
        return;
    };

    let mut action = Action::None;

    let modal = egui::Modal::new(egui::Id::new("program-uninstall-confirm")).show(
        ctx,
        |ui| {
            ui.set_min_width(560.0);
            ui.set_max_width(640.0);

            modal_header(
                ui,
                ph::PACKAGE,
                "Запустить деинсталлятор?",
                "Будет запущен официальный деинсталлятор программы.",
                ACCENT,
            );

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                modal_stat(ui, "Программа", &shorten(&entry.display_name, 42), TEXT);
                ui.add_space(24.0);
                modal_stat(
                    ui,
                    "Размер",
                    &format::bytes(u64::from(entry.size_kb) * 1024),
                    TEXT,
                );
            });

            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{} · {} · {}",
                    entry.publisher,
                    entry.version,
                    entry.source.label()
                ))
                .font(font(9.5))
                .color(TEXT_MUTED),
            );

            ui.add_space(12.0);
            Frame::new()
                .fill(SURFACE_2)
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Команда")
                            .font(font(9.0))
                            .color(TEXT_DIM),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&entry.command_display)
                                .font(FontId::new(9.0, FontFamily::Monospace))
                                .color(TEXT_MUTED),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&entry.command_display);
                });

            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "ChipClean не удаляет файлы и ключи программы напрямую. Дальнейшие действия выполняет сам деинсталлятор; после завершения нажмите «Обновить» и проверьте оставшиеся следы.",
                )
                .font(font(10.0))
                .color(TEXT_MUTED),
            );

            if entry.quiet {
                ui.add_space(8.0);
                warning_box(
                    ui,
                    WARNING,
                    "Будет использован тихий деинсталлятор. Он может работать без окон и дополнительных подтверждений.",
                );
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if widgets::primary_button(
                            ui,
                            "Запустить",
                            ph::ROCKET_LAUNCH,
                            entry.command.is_some(),
                        )
                        .clicked()
                            && entry.command.is_some()
                        {
                            action = Action::Start;
                        }
                        ui.add_space(8.0);
                        if widgets::secondary_button(ui, "Отмена").clicked() {
                            action = Action::Cancel;
                        }
                    },
                );
            });
        },
    );

    if action == Action::None && modal.should_close() {
        state.cancel_uninstall();
        return;
    }

    match action {
        Action::None => {}
        Action::Cancel => state.cancel_uninstall(),
        Action::Start => state.confirm_uninstall(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    None,
    Cancel,
    Start,
}

fn modal_header(ui: &mut Ui, icon: &str, title: &str, subtitle: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(11),
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30),
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::new(21.0, FontFamily::Proportional),
            color,
        );

        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(font_semibold(17.0)).color(TEXT));
            ui.add_space(2.0);
            ui.label(RichText::new(subtitle).font(font(10.5)).color(TEXT_MUTED));
        });
    });
}

fn modal_stat(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).font(font(9.0)).color(TEXT_DIM));
        ui.label(RichText::new(value).font(font_semibold(14.0)).color(color));
    });
}

fn warning_box(ui: &mut Ui, color: Color32, text: &str) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            20,
        ))
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::WARNING)
                        .font(FontId::new(13.0, FontFamily::Proportional))
                        .color(color),
                );
                ui.label(RichText::new(text).font(font(9.5)).color(color));
            });
        });
}

fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut shortened: String = text.chars().take(max_chars).collect();
    shortened.push('…');
    shortened
}
