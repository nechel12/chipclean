use chrono::{DateTime, Local, Utc};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, RichText, Sense,
    Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        rollback::{JournalEvent, RegistryBackup},
        rollback_state::{RestoreRequest, RollbackState},
        startup::StartupEntry,
        traces::QuarantinedTrace,
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut RollbackState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Rollback);

    egui::ScrollArea::vertical()
        .id_salt("rollback-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            info_card(ui);
            ui.add_space(14.0);
            summary(ui, state);
            ui.add_space(14.0);

            if !state.inventory.errors.is_empty() {
                errors(ui, state);
                ui.add_space(14.0);
            }

            registry_section(ui, state);
            ui.add_space(16.0);
            traces_section(ui, state);
            ui.add_space(16.0);
            startup_section(ui, state);
            ui.add_space(16.0);
            journal_section(ui, state);
        });

    if state.pending.is_some() {
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
                    ph::CLOCK_COUNTER_CLOCKWISE,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Единый центр отката")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Восстановление реестра из .reg-бэкапов, возврат объектов из карантина следов и включение отключённой автозагрузки. Перед импортом .reg автоматически сохраняются страховые копии затронутых разделов.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "Undo", WARNING);
                    },
                );
            });
        });
}

fn summary(ui: &mut Ui, state: &mut RollbackState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} .reg-бэкапов · {} в карантине · {} отключено",
                            state.inventory.registry.len(),
                            state.inventory.traces.len(),
                            state.inventory.startup.len()
                        ))
                        .font(font_semibold(15.0))
                        .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "Записей журнала: {}",
                            state.inventory.journal.len()
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
                    if state.is_busy() {
                        ui.spinner();
                        ui.add_space(6.0);
                        if let Some(label) = state.busy_label() {
                            ui.label(RichText::new(label).font(font(10.0)).color(TEXT_MUTED));
                        }
                    } else if widgets::secondary_button_icon(ui, "Обновить", ph::ARROW_CLOCKWISE)
                        .clicked()
                    {
                        state.start_scan();
                    }
                });
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [360.0, 32.0],
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("Поиск по бэкапам, карантину, автозагрузке и журналу"),
                );
                ui.add_space(8.0);
                widgets::inline_hint(
                    ui,
                    ph::SHIELD_CHECK,
                    "Восстановление всегда начинается с подтверждения.",
                    TEXT_DIM,
                );
            });

            if state.inventory.truncated {
                ui.add_space(8.0);
                widgets::inline_hint(
                    ui,
                    ph::WARNING,
                    "Показаны не все .reg-бэкапы: сработал лимит списка.",
                    WARNING,
                );
            }
        });
}

fn errors(ui: &mut Ui, state: &RollbackState) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            WARNING.r(),
            WARNING.g(),
            WARNING.b(),
            16,
        ))
        .stroke(Stroke::new(1.0, WARNING))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            for error in &state.inventory.errors {
                widgets::inline_hint(ui, ph::WARNING, error, WARNING);
            }
        });
}

fn registry_section(ui: &mut Ui, state: &mut RollbackState) {
    section_title(
        ui,
        ph::FLOPPY_DISK,
        "Реестр · .reg-бэкапы",
        state.inventory.registry.len(),
    );

    let query = state.query.trim().to_lowercase();
    let mut restore = None;
    let mut open = None;

    for (index, backup) in state.inventory.registry.iter().enumerate() {
        if !matches_registry(backup, &query) {
            continue;
        }
        match registry_row(ui, backup, index) {
            RowAction::Restore => restore = Some(index),
            RowAction::OpenFolder => open = backup.path.parent().map(|path| path.to_path_buf()),
            RowAction::None => {}
        }
        ui.add_space(6.0);
    }

    if state.inventory.registry.is_empty() {
        empty_hint(
            ui,
            "Пока нет .reg-бэкапов. Они появятся после чисток и правок реестра.",
        );
    } else if restore.is_none()
        && !state
            .inventory
            .registry
            .iter()
            .any(|backup| matches_registry(backup, &query))
    {
        empty_hint(ui, "По запросу бэкапы не найдены.");
    }

    if let Some(index) = restore
        && let Some(backup) = state.inventory.registry.get(index).cloned()
    {
        state.request_registry(&backup);
    }
    if let Some(path) = open
        && let Err(err) = config::open_dir(&path)
    {
        tracing::warn!(%err, path = %path.display(), "failed to open backup folder");
    }
}

fn traces_section(ui: &mut Ui, state: &mut RollbackState) {
    section_title(
        ui,
        ph::FOOTPRINTS,
        "Карантин следов",
        state.inventory.traces.len(),
    );

    let query = state.query.trim().to_lowercase();
    let mut restore = None;
    let mut shown = false;

    for (index, entry) in state.inventory.traces.iter().enumerate() {
        if !matches_trace(entry, &query) {
            continue;
        }
        shown = true;
        if trace_row(ui, entry, index) {
            restore = Some(index);
        }
        ui.add_space(6.0);
    }

    if state.inventory.traces.is_empty() {
        empty_hint(ui, "Карантин следов пуст.");
    } else if !shown {
        empty_hint(ui, "По запросу объекты карантина не найдены.");
    }

    if let Some(index) = restore
        && let Some(entry) = state.inventory.traces.get(index).cloned()
    {
        state.request_trace(&entry);
    }
}

fn startup_section(ui: &mut Ui, state: &mut RollbackState) {
    section_title(
        ui,
        ph::ROCKET_LAUNCH,
        "Отключённая автозагрузка",
        state.inventory.startup.len(),
    );

    let query = state.query.trim().to_lowercase();
    let mut restore = None;
    let mut shown = false;

    for (index, entry) in state.inventory.startup.iter().enumerate() {
        if !matches_startup(entry, &query) {
            continue;
        }
        shown = true;
        if startup_row(ui, entry, index) {
            restore = Some(index);
        }
        ui.add_space(6.0);
    }

    if state.inventory.startup.is_empty() {
        empty_hint(ui, "Отключённых записей автозагрузки нет.");
    } else if !shown {
        empty_hint(ui, "По запросу записи не найдены.");
    }

    if let Some(index) = restore
        && let Some(entry) = state.inventory.startup.get(index).cloned()
    {
        state.request_startup(&entry);
    }
}

fn journal_section(ui: &mut Ui, state: &RollbackState) {
    section_title(
        ui,
        ph::NOTEBOOK,
        "Последние операции · журнал",
        state.inventory.journal.len(),
    );

    let query = state.query.trim().to_lowercase();
    let mut shown = false;

    for event in &state.inventory.journal {
        if !matches_journal(event, &query) {
            continue;
        }
        shown = true;
        journal_row(ui, event);
        ui.add_space(4.0);
    }

    if state.inventory.journal.is_empty() {
        empty_hint(ui, "Журнал пока пуст.");
    } else if !shown {
        empty_hint(ui, "По запросу записи журнала не найдены.");
    }
}

fn section_title(ui: &mut Ui, icon: &str, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icon)
                .font(FontId::new(15.0, FontFamily::Proportional))
                .color(ACCENT),
        );
        ui.label(RichText::new(title).font(font_semibold(12.0)).color(TEXT));
        ui.label(
            RichText::new(count.to_string())
                .font(font(10.0))
                .color(TEXT_DIM),
        );
    });
    ui.add_space(6.0);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowAction {
    None,
    Restore,
    OpenFolder,
}

fn registry_row(ui: &mut Ui, backup: &RegistryBackup, index: usize) -> RowAction {
    let mut action = RowAction::None;

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::ARCHIVE_BOX)
                        .font(FontId::new(17.0, FontFamily::Proportional))
                        .color(TEXT_MUTED),
                );
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(shorten(&backup.label, 78))
                                .font(font_semibold(10.5))
                                .color(TEXT),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&backup.label);

                    let mut details = vec![format::bytes(backup.size)];
                    if let Some(created) = backup.created_at.as_ref() {
                        details.push(format_date(created));
                    }
                    details.push(format!("разделов: {}", backup.targets.len()));
                    ui.label(
                        RichText::new(details.join(" · "))
                            .font(font(9.5))
                            .color(TEXT_MUTED),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(backup.target_summary())
                                .font(FontId::new(8.5, FontFamily::Monospace))
                                .color(TEXT_DIM),
                        )
                        .truncate(),
                    )
                    .on_hover_text(
                        backup
                            .targets
                            .iter()
                            .map(|target| format!("{}\\{}", target.hive.short(), target.path))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    if backup.truncated {
                        ui.label(
                            RichText::new("Список разделов сокращён")
                                .font(font(8.5))
                                .color(WARNING),
                        );
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if compact_button(
                        ui,
                        "Восстановить",
                        ph::CLOCK_COUNTER_CLOCKWISE,
                        ACCENT,
                        true,
                        ui.id().with(("rollback-restore", index)),
                    )
                    .clicked()
                    {
                        action = RowAction::Restore;
                    }
                    ui.add_space(6.0);
                    if icon_button(
                        ui,
                        ph::FOLDER_OPEN,
                        ui.id().with(("rollback-folder", index)),
                    )
                    .on_hover_text("Открыть папку бэкапа")
                    .clicked()
                    {
                        action = RowAction::OpenFolder;
                    }
                });
            });
        });

    action
}

fn trace_row(ui: &mut Ui, entry: &QuarantinedTrace, index: usize) -> bool {
    let mut clicked = false;

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::FOOTPRINTS)
                        .font(FontId::new(17.0, FontFamily::Proportional))
                        .color(TEXT_MUTED),
                );
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(shorten(&entry.title, 70))
                            .font(font_semibold(10.5))
                            .color(TEXT),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(entry.original_path.display().to_string())
                                .font(FontId::new(8.5, FontFamily::Monospace))
                                .color(TEXT_DIM),
                        )
                        .truncate(),
                    )
                    .on_hover_text(entry.original_path.display().to_string());
                    ui.label(
                        RichText::new(format!("В карантине с {}", entry.quarantined_at))
                            .font(font(9.0))
                            .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if compact_button(
                        ui,
                        "Вернуть",
                        ph::ARROW_COUNTER_CLOCKWISE,
                        ACCENT,
                        true,
                        ui.id().with(("rollback-trace", index)),
                    )
                    .clicked()
                    {
                        clicked = true;
                    }
                });
            });
        });

    clicked
}

fn startup_row(ui: &mut Ui, entry: &StartupEntry, index: usize) -> bool {
    let mut clicked = false;

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::ROCKET_LAUNCH)
                        .font(FontId::new(17.0, FontFamily::Proportional))
                        .color(TEXT_MUTED),
                );
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(shorten(&entry.title, 70))
                            .font(font_semibold(10.5))
                            .color(TEXT),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(shorten(&entry.command, 90))
                                .font(FontId::new(8.5, FontFamily::Monospace))
                                .color(TEXT_DIM),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&entry.command);
                    ui.label(
                        RichText::new(entry.source.label())
                            .font(font(9.0))
                            .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if compact_button(
                        ui,
                        "Включить",
                        ph::POWER,
                        ACCENT,
                        true,
                        ui.id().with(("rollback-startup", index)),
                    )
                    .clicked()
                    {
                        clicked = true;
                    }
                });
            });
        });

    clicked
}

fn journal_row(ui: &mut Ui, event: &JournalEvent) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let timestamp = event
                    .timestamp
                    .as_ref()
                    .map(format_date)
                    .unwrap_or_else(|| "—".to_owned());
                ui.label(
                    RichText::new(timestamp)
                        .font(FontId::new(8.5, FontFamily::Monospace))
                        .color(TEXT_DIM),
                );
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&event.summary)
                            .font(font(10.0))
                            .color(TEXT_MUTED),
                    );
                    if let Some(detail) = &event.detail {
                        ui.add(
                            egui::Label::new(
                                RichText::new(shorten(detail, 110))
                                    .font(FontId::new(8.0, FontFamily::Monospace))
                                    .color(TEXT_DIM),
                            )
                            .truncate(),
                        )
                        .on_hover_text(detail);
                    }
                    if let Some(backup) = &event.backup {
                        ui.label(
                            RichText::new(format!(
                                "Бэкап: {}",
                                shorten(&backup.display().to_string(), 100)
                            ))
                            .font(FontId::new(8.0, FontFamily::Monospace))
                            .color(TEXT_DIM),
                        );
                    }
                });
            });
        });
}

fn compact_button(
    ui: &mut Ui,
    label: &str,
    icon: &str,
    color: Color32,
    enabled: bool,
    id: egui::Id,
) -> egui::Response {
    let width = 132.0_f32.min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::hover());
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(7),
        if enabled && response.hovered() {
            ACCENT_SOFT
        } else {
            SURFACE_3
        },
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{icon}  {label}"),
        font(9.5),
        if enabled { color } else { TEXT_DIM },
    );

    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    widgets::focus_ring(ui, &response, rect, 7);
    response
}

fn icon_button(ui: &mut Ui, icon: &str, id: egui::Id) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(7),
        if response.hovered() {
            ACCENT_SOFT
        } else {
            SURFACE_3
        },
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::new(14.0, FontFamily::Proportional),
        TEXT_MUTED,
    );
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    widgets::focus_ring(ui, &response, rect, 7);
    response
}

fn empty_hint(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            widgets::inline_hint(ui, ph::INFO, text, TEXT_DIM);
        });
}

fn matches_registry(backup: &RegistryBackup, query: &str) -> bool {
    query.is_empty()
        || backup.label.to_lowercase().contains(query)
        || backup.group.to_lowercase().contains(query)
        || backup
            .targets
            .iter()
            .any(|target| target.path.to_lowercase().contains(query))
}

fn matches_trace(entry: &QuarantinedTrace, query: &str) -> bool {
    query.is_empty()
        || entry.title.to_lowercase().contains(query)
        || entry.detail.to_lowercase().contains(query)
        || entry
            .original_path
            .display()
            .to_string()
            .to_lowercase()
            .contains(query)
}

fn matches_startup(entry: &StartupEntry, query: &str) -> bool {
    query.is_empty()
        || entry.title.to_lowercase().contains(query)
        || entry.command.to_lowercase().contains(query)
        || entry.source.label().to_lowercase().contains(query)
}

fn matches_journal(event: &JournalEvent, query: &str) -> bool {
    query.is_empty()
        || event.summary.to_lowercase().contains(query)
        || event
            .detail
            .as_deref()
            .is_some_and(|detail| detail.to_lowercase().contains(query))
}

fn confirmation_modal(ctx: &egui::Context, state: &mut RollbackState) {
    let Some(request) = state.pending.clone() else {
        return;
    };
    let mut action = Action::None;

    let modal = egui::Modal::new(egui::Id::new("rollback-confirm")).show(ctx, |ui| {
        ui.set_min_width(560.0);
        ui.set_max_width(660.0);

        modal_header(
            ui,
            match &request {
                RestoreRequest::Registry(_) => ph::FLOPPY_DISK,
                RestoreRequest::Trace(_) => ph::FOOTPRINTS,
                RestoreRequest::Startup(_) => ph::ROCKET_LAUNCH,
            },
            "Выполнить откат?",
            &format!("Источник: {}", request.kind_label()),
            ACCENT,
        );

        ui.add_space(14.0);
        ui.label(
            RichText::new(shorten(&request.title(), 90))
                .font(font_semibold(12.0))
                .color(TEXT),
        );
        ui.add_space(8.0);

        Frame::new()
            .fill(BG)
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("rollback-confirm-details")
                    .max_height(180.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in request.details() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(line)
                                        .font(FontId::new(9.0, FontFamily::Monospace))
                                        .color(TEXT_MUTED),
                                )
                                .truncate(),
                            );
                        }
                    });
            });

        if matches!(request, RestoreRequest::Registry(_)) {
            ui.add_space(10.0);
            warning_box(
                ui,
                WARNING,
                "Импорт .reg перезапишет существующие значения и создаст отсутствующие разделы. Перед импортом ChipClean автоматически сохранит страховые копии затронутых разделов в каталог бэкапов.",
            );
            ui.add_space(8.0);
            widgets::confirmation_checkbox(
                ui,
                &mut state.confirm_ack,
                "Я понимаю, что реестр будет изменён, и страховые копии уже будут созданы",
            );
        } else {
            ui.add_space(10.0);
            widgets::inline_hint(
                ui,
                ph::SHIELD_CHECK,
                "Операция обратима: объект снова можно будет отключить или удалить.",
                TEXT_DIM,
            );
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = !request.needs_ack() || state.confirm_ack;
                if widgets::primary_button(ui, "Восстановить", ph::CLOCK_COUNTER_CLOCKWISE, ready)
                    .clicked()
                    && ready
                {
                    action = Action::Start;
                }
                ui.add_space(8.0);
                if widgets::secondary_button(ui, "Отмена").clicked() {
                    action = Action::Cancel;
                }
            });
        });
    });

    if action == Action::None && modal.should_close() {
        state.cancel_restore();
        return;
    }

    match action {
        Action::None => {}
        Action::Cancel => state.cancel_restore(),
        Action::Start => state.confirm_restore(),
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

fn format_date(value: &DateTime<Utc>) -> String {
    value
        .with_timezone(&Local)
        .format("%d.%m.%Y %H:%M:%S")
        .to_string()
}

fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut shortened: String = text.chars().take(max_chars).collect();
    shortened.push('…');
    shortened
}
