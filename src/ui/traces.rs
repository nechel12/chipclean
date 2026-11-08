use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui,
    Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    core::traces::{QuarantinedTrace, TraceCategoryId, TraceEntry, TraceKind},
    core::traces_state::{TraceAction, TraceScanState, TracesState},
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut TracesState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Traces);

    egui::ScrollArea::vertical()
        .id_salt("traces-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            info_card(ui);
            if let Some(program) = &state.focus_program {
                ui.add_space(10.0);
                widgets::inline_hint(
                    ui,
                    ph::FOOTPRINTS,
                    &format!("Следы после удаления «{program}»"),
                    ACCENT,
                );
            }
            ui.add_space(14.0);
            summary(ui, state);
            ui.add_space(14.0);
            categories(ui, state);
            ui.add_space(14.0);
            findings(ui, state);
            ui.add_space(14.0);
            quarantine(ui, state);
        });

    if state.pending_action.is_some() {
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
                    ph::FOOTPRINTS,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Следы удалённых программ")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Битые ярлыки, папки Program Files без записи Uninstall и тестовый глубокий поиск по AppData/ProgramData. Можно переместить в карантин или удалить сразу после подтверждения.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "3 проверки", WARNING);
                    },
                );
            });
        });
}

fn summary(ui: &mut Ui, state: &mut TracesState) {
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
                            "Выбрано {} из {} · найдено: {}",
                            state.selected.len(),
                            state.categories.len(),
                            state.entries.len()
                        ))
                        .font(font_semibold(15.0))
                        .color(TEXT),
                    );
                    ui.add_space(2.0);
                    status_line(ui, state);

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
                    if state.is_scanning() {
                        if widgets::primary_button(ui, "Остановить", ph::X, true).clicked()
                        {
                            state.cancel_scan();
                        }
                    } else if state.can_process() {
                        if widgets::primary_button(ui, "В карантин", ph::TRASH, true).clicked()
                        {
                            state.request_action(TraceAction::Quarantine);
                        }
                        ui.add_space(8.0);
                        if widgets::danger_button(ui, "Удалить сразу", true).clicked() {
                            state.request_action(TraceAction::Delete);
                        }
                        ui.add_space(8.0);
                        if widgets::secondary_button_icon(
                            ui,
                            "Проверить снова",
                            ph::ARROW_CLOCKWISE,
                        )
                        .clicked()
                        {
                            state.start_scan();
                        }
                    } else {
                        let enabled = !state.selected.is_empty();
                        if widgets::primary_button(
                            ui,
                            "Проверить следы",
                            ph::MAGNIFYING_GLASS,
                            enabled,
                        )
                        .clicked()
                            && enabled
                        {
                            state.start_scan();
                        }
                    }
                });
            });

            if state.is_scanning() {
                ui.add_space(14.0);
                let (done, total, current) = state.scan_progress();
                widgets::progress(ui, done as f32 / total.max(1) as f32, 5.0, ACCENT);
                ui.add_space(6.0);
                let current = current
                    .and_then(|id| {
                        state
                            .categories
                            .iter()
                            .find(|category| category.id == id)
                            .map(|category| category.title)
                    })
                    .unwrap_or("Подготовка…");
                ui.label(
                    RichText::new(format!("Поиск следов: {done} из {total} · {current}"))
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                );
            }

            if state.is_mutating() {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Обработка следов…")
                            .font(font(10.0))
                            .color(TEXT_MUTED),
                    );
                });
            }

            if !state.entries.is_empty()
                && state.selected != state.scanned_categories
                && !state.is_scanning()
            {
                ui.add_space(8.0);
                widgets::inline_hint(
                    ui,
                    ph::INFO,
                    "Выбор изменился. Запустите проверку заново, чтобы обновить список.",
                    TEXT_MUTED,
                );
            }

            if state
                .categories
                .iter()
                .any(|category| category.experimental && state.selected.contains(&category.id))
            {
                ui.add_space(8.0);
                widgets::inline_hint(
                    ui,
                    ph::WARNING,
                    "Глубокий поиск — тестовая эвристика AppData/ProgramData. Возможны ложные срабатывания; проверяйте каждый путь.",
                    WARNING,
                );
            }

            ui.add_space(8.0);
            widgets::inline_hint(
                ui,
                ph::SHIELD_CHECK,
                "«В карантин» можно вернуть; «Удалить сразу» — безвозвратно.",
                TEXT_DIM,
            );
        });
}

fn status_line(ui: &mut Ui, state: &TracesState) {
    match &state.scan {
        TraceScanState::Idle => {
            ui.label(
                RichText::new("Проверка ещё не запускалась")
                    .font(font(10.5))
                    .color(TEXT_DIM),
            );
        }
        TraceScanState::Scanning {
            done,
            total,
            started,
            ..
        } => {
            ui.label(
                RichText::new(format!(
                    "Идёт поиск следов · {done} из {total} · {}",
                    format::elapsed(started.elapsed())
                ))
                .font(font(10.5))
                .color(TEXT_MUTED),
            );
        }
        TraceScanState::Done {
            findings,
            elapsed,
            cancelled,
        } => {
            let text = if *cancelled {
                format!(
                    "Проверка остановлена · найдено: {} · {}",
                    findings,
                    format::elapsed(*elapsed)
                )
            } else if *findings == 0 {
                format!("Следов не найдено · {}", format::elapsed(*elapsed))
            } else {
                format!("Найдено: {} · {}", findings, format::elapsed(*elapsed))
            };
            ui.label(RichText::new(text).font(font(10.5)).color(if *cancelled {
                WARNING
            } else {
                TEXT_MUTED
            }));
        }
        TraceScanState::Failed(message) => {
            ui.label(RichText::new(message).font(font(10.5)).color(ACCENT));
        }
    }
}

fn categories(ui: &mut Ui, state: &mut TracesState) {
    ui.label(
        RichText::new("Разделы поиска")
            .font(font_semibold(12.0))
            .color(TEXT),
    );
    ui.add_space(3.0);
    ui.label(
        RichText::new("Отключённые разделы не сканируются и не попадут в карантин.")
            .font(font(9.5))
            .color(TEXT_DIM),
    );
    ui.add_space(6.0);

    let busy = state.is_scanning() || state.is_mutating();
    let scanned = matches!(state.scan, TraceScanState::Done { .. });
    let categories = &state.categories;
    let selected = &mut state.selected;
    let entries = &state.entries;

    for category in categories {
        let checked = selected.contains(&category.id);
        let count = if scanned {
            Some(
                entries
                    .iter()
                    .filter(|entry| entry.category == category.id)
                    .count()
                    .to_string(),
            )
        } else {
            None
        };

        let response = widgets::selectable_row(
            ui,
            category.icon,
            category.title,
            category.summary,
            checked,
            !busy,
            widgets::risk_color(category.risk),
            count.as_deref(),
        );

        if response.clicked() && !busy {
            if checked {
                selected.remove(&category.id);
            } else {
                selected.insert(category.id);
            }
        }
        ui.add_space(2.0);
    }
}

fn findings(ui: &mut Ui, state: &TracesState) {
    if !matches!(state.scan, TraceScanState::Done { .. }) {
        return;
    }

    if state.entries.is_empty() {
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(ph::CHECK_CIRCLE)
                            .font(FontId::new(18.0, FontFamily::Proportional))
                            .color(SUCCESS),
                    );
                    ui.label(
                        RichText::new("Следов не найдено. Чисто.")
                            .font(font(11.0))
                            .color(TEXT_MUTED),
                    );
                });
            });
        return;
    }

    ui.label(
        RichText::new(format!("Найденные следы · {}", state.entries.len()))
            .font(font_semibold(12.0))
            .color(TEXT),
    );
    ui.add_space(6.0);

    for category in &state.categories {
        let items: Vec<_> = state
            .entries
            .iter()
            .filter(|entry| entry.category == category.id)
            .collect();
        if items.is_empty() {
            continue;
        }

        ui.label(
            RichText::new(format!("{} · {}", category.title, items.len()))
                .font(font_semibold(10.5))
                .color(widgets::risk_color(category.risk)),
        );
        ui.add_space(4.0);

        for entry in items {
            trace_row(ui, entry);
            ui.add_space(5.0);
        }
        ui.add_space(8.0);
    }
}

fn trace_row(ui: &mut Ui, entry: &TraceEntry) {
    let tooltip = match &entry.kind {
        TraceKind::Shortcut { target } => format!("Цель: {}", target.display()),
        TraceKind::OrphanFolder { evidence } => {
            format!("Деинсталлятор: {}", evidence.display())
        }
        TraceKind::DataFolder { reason } => format!("Эвристика: {reason}"),
    };

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(category_icon(entry.category))
                        .font(FontId::new(16.0, FontFamily::Proportional))
                        .color(widgets::risk_color(category_risk(entry.category))),
                );
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&entry.title)
                            .font(font_semibold(11.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(&entry.detail)
                            .font(font(9.5))
                            .color(TEXT_MUTED),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(entry.path.display().to_string())
                                .font(FontId::new(8.5, FontFamily::Monospace))
                                .color(TEXT_DIM),
                        )
                        .truncate(),
                    )
                    .on_hover_text(entry.path.display().to_string());
                });
            });
        })
        .response
        .on_hover_text(tooltip);
}

fn quarantine(ui: &mut Ui, state: &mut TracesState) {
    if state.quarantined.is_empty() {
        return;
    }

    ui.label(
        RichText::new(format!("В карантине · {}", state.quarantined.len()))
            .font(font_semibold(12.0))
            .color(TEXT),
    );
    ui.add_space(6.0);

    let mut restore_index = None;

    for (index, entry) in state.quarantined.iter().enumerate() {
        if quarantine_row(ui, entry, index) {
            restore_index = Some(index);
        }
        ui.add_space(5.0);
    }

    if let Some(index) = restore_index {
        state.restore(index);
    }
}

fn quarantine_row(ui: &mut Ui, entry: &QuarantinedTrace, index: usize) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(9), SURFACE);

    ui.painter().text(
        rect.left_center() + Vec2::new(14.0, -8.0),
        Align2::LEFT_CENTER,
        shorten(&entry.title, 54),
        font_semibold(10.5),
        TEXT_MUTED,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(14.0, 9.0),
        Align2::LEFT_CENTER,
        shorten(&entry.original_path.display().to_string(), 90),
        FontId::new(8.0, FontFamily::Monospace),
        TEXT_DIM,
    );

    let button_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 58.0, rect.center().y),
        Vec2::new(92.0, 28.0),
    );
    let response = ui.interact(
        button_rect,
        ui.id().with(("trace-restore", index)),
        Sense::click(),
    );
    ui.painter().rect_filled(
        button_rect,
        CornerRadius::same(7),
        if response.hovered() {
            ACCENT_SOFT
        } else {
            SURFACE_3
        },
    );
    ui.painter().text(
        button_rect.center(),
        Align2::CENTER_CENTER,
        "Вернуть",
        font(9.5),
        TEXT,
    );

    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Вернуть {}", entry.title),
        )
    });
    widgets::focus_ring(ui, &response, button_rect, 7);

    response.clicked()
}

fn confirmation_modal(ctx: &egui::Context, state: &mut TracesState) {
    let Some(disposition) = state.pending_action else {
        return;
    };
    let entries = state.selected_entries();
    let needs_ack = state.needs_ack(disposition);
    let delete = disposition == TraceAction::Delete;
    let finding_categories = entries
        .iter()
        .map(|entry| entry.category)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let mut action = Action::None;

    let modal = egui::Modal::new(egui::Id::new("traces-confirm")).show(ctx, |ui| {
        ui.set_min_width(580.0);
        ui.set_max_width(660.0);

        modal_header(
            ui,
            if delete { ph::WARNING } else { ph::FOOTPRINTS },
            if delete {
                "Удалить безвозвратно?"
            } else {
                "Переместить в карантин?"
            },
            if delete {
                "Восстановление будет невозможно."
            } else {
                "Объекты можно будет вернуть из раздела «В карантине»."
            },
            ACCENT,
        );

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            modal_stat(ui, "Объектов", &entries.len().to_string(), TEXT);
            ui.add_space(24.0);
            modal_stat(ui, "Категорий", &finding_categories.to_string(), TEXT);
        });

        ui.add_space(10.0);
        Frame::new()
            .fill(BG)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("traces-confirm-list")
                    .max_height(200.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in &entries {
                            modal_trace_row(ui, entry);
                        }
                    });
            });

        ui.add_space(10.0);
        warning_box(
            ui,
            if delete { ACCENT } else { WARNING },
            if delete {
                "Файлы и папки будут удалены навсегда. Восстановить их будет нельзя."
            } else {
                "Файлы и папки будут перемещены в карантин. Для папок это может занять время; вернуть их можно кнопкой «Вернуть»."
            },
        );

        if needs_ack {
            ui.add_space(8.0);
            widgets::confirmation_checkbox(
                ui,
                &mut state.confirm_ack,
                if delete {
                    "Я понимаю, что восстановление невозможно"
                } else {
                    "Я понимаю, что папки будут перемещены в карантин"
                },
            );
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = !needs_ack || state.confirm_ack;
                if widgets::primary_button(
                    ui,
                    if delete { "Удалить навсегда" } else { "В карантин" },
                    ph::TRASH,
                    ready,
                )
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
        state.cancel_action();
        return;
    }

    match action {
        Action::None => {}
        Action::Cancel => state.cancel_action(),
        Action::Start => state.confirm_action(),
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
        ui.label(RichText::new(value).font(font_semibold(15.0)).color(color));
    });
}

fn modal_trace_row(ui: &mut Ui, entry: &TraceEntry) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y - 7.0),
        Align2::LEFT_CENTER,
        shorten(&entry.title, 60),
        font_semibold(10.0),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y + 7.0),
        Align2::LEFT_CENTER,
        shorten(&entry.path.display().to_string(), 82),
        FontId::new(8.0, FontFamily::Monospace),
        TEXT_DIM,
    );
    ui.add_space(3.0);
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

fn category_icon(id: TraceCategoryId) -> &'static str {
    match id {
        TraceCategoryId::BrokenShortcuts => ph::LINK_SIMPLE_BREAK,
        TraceCategoryId::OrphanedFolders => ph::FOLDER_SIMPLE,
        TraceCategoryId::DeepLeftovers => ph::MAGNIFYING_GLASS_PLUS,
    }
}

fn category_risk(id: TraceCategoryId) -> crate::core::model::Risk {
    match id {
        TraceCategoryId::BrokenShortcuts => crate::core::model::Risk::Caution,
        TraceCategoryId::OrphanedFolders | TraceCategoryId::DeepLeftovers => {
            crate::core::model::Risk::Advanced
        }
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
