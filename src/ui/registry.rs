use std::path::Path;

use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui,
    Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        model::Risk,
        registry::{RegistryCategoryId, RegistryFinding},
        registry_state::{RegistryCleanState, RegistryScanState, RegistryState},
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut RegistryState) {
    common::page_header(ui, Page::Registry);

    info_card(ui);
    ui.add_space(14.0);
    summary(ui, state);
    ui.add_space(14.0);
    categories(ui, state);
    ui.add_space(14.0);
    findings(ui, state);

    if state.pending_confirmation {
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
                    ph::SHIELD_CHECK,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Безопасное подмножество реестра")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Проверяются только записи деинсталляции, автозагрузка и App Paths. Перед удалением создаётся .reg-бэкап; изменённые ключи можно импортировать обратно.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "3 проверки", SUCCESS);
                    },
                );
            });
        });
}

fn summary(ui: &mut Ui, state: &mut RegistryState) {
    let backup_dir = match &state.clean {
        RegistryCleanState::Done { backup_dir, .. } => Some(backup_dir.clone()),
        RegistryCleanState::Cleaning {
            backup_dir: Some(dir),
            ..
        } => Some(dir.clone()),
        _ => None,
    };

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
                            "Выбрано {} из {}",
                            state.selected.len(),
                            state.categories.len()
                        ))
                        .font(font_semibold(15.0))
                        .color(TEXT),
                    );
                    ui.add_space(2.0);
                    status_line(ui, state);
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let scanning = state.is_scanning();
                        let cleaning = state.is_cleaning();

                        if cleaning {
                            if widgets::primary_button(ui, "Остановить", ph::X, true).clicked() {
                                state.cancel_clean();
                            }
                        } else if scanning {
                            if widgets::primary_button(ui, "Остановить", ph::X, true).clicked() {
                                state.cancel_scan();
                            }
                        } else if state.can_clean() {
                            if widgets::primary_button(ui, "Удалить записи", ph::TRASH, true)
                                .clicked()
                            {
                                state.request_cleanup();
                            }
                            ui.add_space(8.0);
                            if widgets::secondary_button_icon(
                                ui,
                                "Проверить снова",
                                ph::MAGNIFYING_GLASS,
                            )
                            .clicked()
                            {
                                state.start_scan();
                            }
                        } else {
                            let enabled = !state.selected.is_empty();
                            let label = if matches!(state.scan, RegistryScanState::Done { .. }) {
                                "Проверить снова"
                            } else {
                                "Проверить реестр"
                            };
                            if widgets::primary_button(
                                ui,
                                label,
                                ph::MAGNIFYING_GLASS,
                                enabled,
                            )
                            .clicked()
                                && enabled
                            {
                                state.start_scan();
                            }
                        }
                    },
                );
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
                    RichText::new(format!("Проверка реестра: {done} из {total} · {current}"))
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                );
            } else if state.is_cleaning() {
                ui.add_space(14.0);

                if let RegistryCleanState::Cleaning {
                    total,
                    done,
                    deleted,
                    errors,
                    current,
                    started,
                    ..
                } = &state.clean
                {
                    widgets::progress(ui, *done as f32 / (*total).max(1) as f32, 5.0, ACCENT);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "Очистка реестра: {done} из {total} · {} · {}",
                            shorten(current, 70),
                            format::elapsed(started.elapsed())
                        ))
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(2.0);
                    let mut text = format!("Удалено: {deleted}");
                    if *errors > 0 {
                        text.push_str(&format!(" · ошибок: {errors}"));
                    }
                    ui.label(
                        RichText::new(text)
                            .font(font(10.0))
                            .color(if *errors > 0 { WARNING } else { TEXT_MUTED }),
                    );
                }
            }

            ui.add_space(10.0);

            if !state.is_cleaning() {
                widgets::inline_hint(
                    ui,
                    ph::SHIELD_CHECK,
                    "Перед изменением реестра создаётся .reg-бэкап в папке данных ChipClean.",
                    TEXT_DIM,
                );

                if !state.findings.is_empty()
                    && state.selected != state.scanned_categories
                    && !state.is_scanning()
                {
                    widgets::inline_hint(
                        ui,
                        ph::INFO,
                        "Выбор изменился. Запустите проверку заново, чтобы обновить список записей.",
                        TEXT_MUTED,
                    );
                }
            }

            if let Some(dir) = backup_dir {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Бэкап: {}", shorten(&dir.display().to_string(), 72)))
                            .font(font(9.0))
                            .color(TEXT_DIM),
                    )
                    .on_hover_text(dir.display().to_string());
                    if widgets::secondary_button(ui, "Открыть папку").clicked() {
                        open_dir(&dir);
                    }
                });
            }
        });
}

fn status_line(ui: &mut Ui, state: &RegistryState) {
    match &state.clean {
        RegistryCleanState::Cleaning { done, total, .. } => {
            ui.label(
                RichText::new(format!("Идёт очистка реестра · {done} из {total}"))
                    .font(font(10.5))
                    .color(ACCENT),
            );
        }
        RegistryCleanState::Done {
            deleted,
            errors,
            elapsed,
            cancelled,
            ..
        } => {
            let mut text = if *cancelled {
                format!(
                    "Очистка остановлена · удалено {} · {}",
                    format::records(*deleted as u64),
                    format::elapsed(*elapsed)
                )
            } else {
                format!(
                    "Удалено {} · {}",
                    format::records(*deleted as u64),
                    format::elapsed(*elapsed)
                )
            };
            if *errors > 0 {
                text.push_str(&format!(" · ошибок: {errors}"));
            }
            ui.label(RichText::new(text).font(font(10.5)).color(if *cancelled {
                WARNING
            } else {
                TEXT_MUTED
            }));
        }
        RegistryCleanState::Failed(message) => {
            ui.label(RichText::new(message).font(font(10.5)).color(ACCENT));
        }
        RegistryCleanState::Idle => match &state.scan {
            RegistryScanState::Idle => {
                ui.label(
                    RichText::new("Проверка ещё не запускалась")
                        .font(font(10.5))
                        .color(TEXT_DIM),
                );
            }
            RegistryScanState::Scanning {
                done,
                total,
                started,
                ..
            } => {
                ui.label(
                    RichText::new(format!(
                        "Идёт проверка реестра · {done} из {total} · {}",
                        format::elapsed(started.elapsed())
                    ))
                    .font(font(10.5))
                    .color(TEXT_MUTED),
                );
            }
            RegistryScanState::Done {
                findings,
                elapsed,
                cancelled,
            } => {
                let text = if *cancelled {
                    format!(
                        "Проверка остановлена · найдено {} · {}",
                        format::records(*findings as u64),
                        format::elapsed(*elapsed)
                    )
                } else if *findings == 0 {
                    format!("Проблем не найдено · {}", format::elapsed(*elapsed))
                } else {
                    format!(
                        "Найдено {} · {}",
                        format::records(*findings as u64),
                        format::elapsed(*elapsed)
                    )
                };
                ui.label(RichText::new(text).font(font(10.5)).color(if *cancelled {
                    WARNING
                } else {
                    TEXT_MUTED
                }));
            }
            RegistryScanState::Failed(message) => {
                ui.label(RichText::new(message).font(font(10.5)).color(ACCENT));
            }
        },
    }
}

fn categories(ui: &mut Ui, state: &mut RegistryState) {
    ui.label(
        RichText::new("Разделы проверки")
            .font(font_semibold(12.0))
            .color(TEXT),
    );
    ui.add_space(3.0);
    ui.label(
        RichText::new("Только безопасное подмножество, без агрессивного поиска по всему реестру.")
            .font(font(9.5))
            .color(TEXT_DIM),
    );
    ui.add_space(6.0);

    let busy = state.is_scanning() || state.is_cleaning();
    let scanned = matches!(state.scan, RegistryScanState::Done { .. });
    let categories = &state.categories;
    let selected = &mut state.selected;
    let findings = &state.findings;

    for category in categories {
        let checked = selected.contains(&category.id);
        let count = if scanned {
            Some(
                findings
                    .iter()
                    .filter(|finding| finding.category == category.id)
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

    ui.add_space(4.0);
    ui.label(
        RichText::new("Глубокий поиск путей и неверные ярлыки — следующий этап.")
            .font(font(9.0))
            .color(TEXT_DIM),
    );
}

fn findings(ui: &mut Ui, state: &RegistryState) {
    if !matches!(state.scan, RegistryScanState::Done { .. }) {
        return;
    }

    if state.findings.is_empty() {
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
                        RichText::new("Проблем не найдено. Реестр выглядит чисто.")
                            .font(font(11.0))
                            .color(TEXT_MUTED),
                    );
                });
            });
        return;
    }

    ui.label(
        RichText::new(format!("Найденные записи · {}", state.findings.len()))
            .font(font_semibold(12.0))
            .color(TEXT),
    );
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .id_salt("registry-findings")
        .max_height(300.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for finding in &state.findings {
                finding_row(ui, finding);
                ui.add_space(6.0);
            }
        });
}

fn finding_row(ui: &mut Ui, finding: &RegistryFinding) {
    let risk = category_risk(finding.category);

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(11))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(category_icon(finding.category))
                        .font(FontId::new(16.0, FontFamily::Proportional))
                        .color(widgets::risk_color(risk)),
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&finding.title)
                                .font(font_semibold(11.0))
                                .color(TEXT),
                        );
                        widgets::badge(
                            ui,
                            category_label(finding.category),
                            widgets::risk_color(risk),
                        );
                        widgets::badge(ui, finding.hive.short(), TEXT_DIM);
                    });
                    ui.label(
                        RichText::new(&finding.detail)
                            .font(font(9.5))
                            .color(TEXT_MUTED),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&finding.key_display)
                                .font(FontId::new(8.5, FontFamily::Monospace))
                                .color(TEXT_DIM),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&finding.key_display);
                });
            });
        });
}

fn confirmation_modal(ctx: &egui::Context, state: &mut RegistryState) {
    let findings = state.findings.clone();
    let finding_categories = findings
        .iter()
        .map(|finding| finding.category)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let backups_base = config::backups_dir();
    let mut action = Action::None;

    let modal = egui::Modal::new(egui::Id::new("chipclean-registry-confirm")).show(
        ctx,
        |ui| {
            ui.set_min_width(580.0);
            ui.set_max_width(660.0);

            modal_header(
                ui,
                ph::SHIELD_CHECK,
                "Очистка реестра",
                "Перед удалением будет создан .reg-бэкап.",
                ACCENT,
            );

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                modal_stat(ui, "Записей", &findings.len().to_string(), TEXT);
                ui.add_space(24.0);
                modal_stat(ui, "Разделов", &finding_categories.to_string(), TEXT);
            });

            ui.add_space(10.0);
            Frame::new()
                .fill(SURFACE_2)
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Бэкап будет создан в:")
                            .font(font(9.0))
                            .color(TEXT_DIM),
                    );
                    ui.label(
                        RichText::new(
                            backups_base
                                .join("registry-<дата>-<время>")
                                .display()
                                .to_string(),
                        )
                        .font(FontId::new(9.0, FontFamily::Monospace))
                        .color(TEXT_MUTED),
                    );
                });

            ui.add_space(10.0);
            Frame::new()
                .fill(BG)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("registry-confirm-list")
                        .max_height(200.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for finding in &findings {
                                modal_finding_row(ui, finding);
                            }
                        });
                });

            ui.add_space(10.0);
            warning_box(
                ui,
                ACCENT,
                "Изменения реестра могут повлиять на приложения. Удаляйте только если уверены; бэкап можно импортировать обратно.",
            );

            ui.add_space(10.0);
            widgets::confirmation_checkbox(
                ui,
                &mut state.confirm_ack,
                "Я понимаю и подтверждаю изменение реестра",
            );

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if widgets::primary_button(
                            ui,
                            "Удалить записи",
                            ph::TRASH,
                            state.confirm_ack,
                        )
                        .clicked()
                            && state.confirm_ack
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
        state.cancel_confirmation();
        return;
    }

    match action {
        Action::None => {}
        Action::Cancel => state.cancel_confirmation(),
        Action::Start => {
            if let Err(err) = state.start_clean() {
                state.cancel_confirmation();
                state.clean = RegistryCleanState::Failed(err);
            }
        }
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

fn modal_finding_row(ui: &mut Ui, finding: &RegistryFinding) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());

    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y - 6.0),
        Align2::LEFT_CENTER,
        &finding.title,
        font_semibold(10.0),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y + 7.0),
        Align2::LEFT_CENTER,
        shorten(&finding.key_display, 78),
        FontId::new(8.0, FontFamily::Monospace),
        TEXT_DIM,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 8.0, rect.center().y),
        Align2::RIGHT_CENTER,
        category_label(finding.category),
        font(8.0),
        widgets::risk_color(category_risk(finding.category)),
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

fn category_icon(id: RegistryCategoryId) -> &'static str {
    match id {
        RegistryCategoryId::Uninstall => ph::PACKAGE,
        RegistryCategoryId::Startup => ph::ROCKET_LAUNCH,
        RegistryCategoryId::AppPaths => ph::APP_WINDOW,
    }
}

fn category_risk(id: RegistryCategoryId) -> Risk {
    match id {
        RegistryCategoryId::Uninstall
        | RegistryCategoryId::Startup
        | RegistryCategoryId::AppPaths => Risk::Caution,
    }
}

fn category_label(id: RegistryCategoryId) -> &'static str {
    match id {
        RegistryCategoryId::Uninstall => "Uninstall",
        RegistryCategoryId::Startup => "Автозагрузка",
        RegistryCategoryId::AppPaths => "App Paths",
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

fn open_dir(path: &Path) {
    if let Err(err) = config::open_dir(path) {
        tracing::warn!(%err, path = %path.display(), "failed to open directory");
    }
}
