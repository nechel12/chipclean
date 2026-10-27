use std::collections::BTreeSet;

use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui,
    Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config::Settings,
    core::{
        model::{CategoryId, CleanupCategory, Group},
        scanner::ScanOutcome,
        state::{CleanState, CleanupState, ConfirmStage, ScanState},
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, cleanup: &mut CleanupState, settings: &mut Settings) {
    common::page_header(ui, Page::Cleanup);

    summary(ui, cleanup, settings);

    if cleanup.selected_count() == 0 {
        ui.add_space(12.0);
        widgets::inline_hint(
            ui,
            ph::INFO,
            "Выберите хотя бы один раздел, чтобы запустить сканирование.",
            TEXT_MUTED,
        );
    }

    ui.add_space(12.0);

    {
        let busy = cleanup.is_scanning() || cleanup.is_cleaning();
        let categories = &cleanup.categories;
        let selected = &mut cleanup.selected;
        let results = &cleanup.results;

        egui::ScrollArea::vertical()
            .id_salt("cleanup-categories")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group in [Group::System, Group::Browsers, Group::Personal] {
                    group_header(ui, group, categories, selected);
                    ui.add_space(5.0);

                    let items: Vec<CleanupCategory> = categories
                        .iter()
                        .filter(|category| category.group == group)
                        .cloned()
                        .collect();

                    ui.columns(2, |columns| {
                        for (index, category) in items.iter().enumerate() {
                            let column = &mut columns[index % 2];
                            let checked = selected.contains(&category.id);
                            let outcome = results.get(&category.id);
                            let response =
                                widgets::category_row(column, category, checked, outcome);

                            if response.clicked() && !busy {
                                if checked {
                                    selected.remove(&category.id);
                                } else {
                                    selected.insert(category.id);
                                }
                            }
                        }
                    });

                    ui.add_space(16.0);
                }
            });
    }

    if cleanup.pending_cleanup.is_some() {
        confirmation_modal(ui.ctx(), cleanup, settings.downloads_older_than_days);
    }
}

fn summary(ui: &mut Ui, cleanup: &mut CleanupState, settings: &mut Settings) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let heading = if cleanup.results.is_empty() {
                        format!(
                            "Выбрано {} из {}",
                            cleanup.selected_count(),
                            cleanup.categories.len()
                        )
                    } else {
                        format!(
                            "Выбрано {} из {} · {} по скану",
                            cleanup.selected_count(),
                            cleanup.categories.len(),
                            format::bytes(cleanup.selected_bytes())
                        )
                    };
                    ui.label(
                        RichText::new(heading)
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    status_line(ui, cleanup);
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let scanning = cleanup.is_scanning();
                        let cleaning = cleanup.is_cleaning();
                        let busy = scanning || cleaning;

                        if cleaning {
                            if widgets::primary_button(
                                ui,
                                "Остановить",
                                ph::X,
                                true,
                            )
                            .clicked()
                            {
                                cleanup.cancel_clean();
                            }
                        } else if scanning {
                            if widgets::primary_button(
                                ui,
                                if cleanup.cancel_requested {
                                    "Останавливаем…"
                                } else {
                                    "Остановить"
                                },
                                ph::X,
                                true,
                            )
                            .clicked()
                            {
                                cleanup.cancel_scan();
                            }
                        } else if cleanup.can_clean() {
                            if widgets::primary_button(
                                ui,
                                "Очистить",
                                ph::TRASH,
                                true,
                            )
                            .clicked()
                            {
                                cleanup.request_cleanup();
                            }
                            ui.add_space(8.0);
                            if widgets::secondary_button_icon(
                                ui,
                                "Сканировать",
                                ph::MAGNIFYING_GLASS,
                            )
                            .clicked()
                            {
                                cleanup.start_scan(settings.downloads_older_than_days);
                            }
                        } else {
                            let enabled = cleanup.selected_count() > 0;
                            if widgets::primary_button(
                                ui,
                                "Сканировать",
                                ph::MAGNIFYING_GLASS,
                                enabled,
                            )
                            .clicked()
                                && enabled
                            {
                                cleanup.start_scan(settings.downloads_older_than_days);
                            }
                        }

                        ui.add_space(14.0);
                        let mut max_cleanup = settings.max_cleanup;
                        if widgets::switch(ui, &mut max_cleanup, "Максимальная чистка")
                            && !busy
                        {
                            settings.max_cleanup = max_cleanup;
                            cleanup.set_max_cleanup(max_cleanup);
                        }
                    },
                );
            });

            if cleanup.is_scanning() {
                ui.add_space(14.0);
                let (done, total, current) = cleanup.progress();
                widgets::progress(ui, done as f32 / total.max(1) as f32, 5.0, ACCENT);
                ui.add_space(6.0);

                let current = current
                    .and_then(|id| cleanup.category(id))
                    .map(|category| category.title)
                    .unwrap_or("Подготовка…");
                ui.label(
                    RichText::new(format!("Сканирование: {done} из {total} · {current}"))
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                );
            } else if cleanup.is_cleaning() {
                ui.add_space(14.0);

                if let CleanState::Cleaning {
                    total_categories,
                    done_categories,
                    current,
                    bytes,
                    files,
                    errors,
                    started,
                } = &cleanup.clean
                {
                    widgets::progress(
                        ui,
                        *done_categories as f32 / (*total_categories).max(1) as f32,
                        5.0,
                        ACCENT,
                    );
                    ui.add_space(6.0);

                    let current = current
                        .and_then(|id| cleanup.category(id))
                        .map(|category| category.title)
                        .unwrap_or("Подготовка…");
                    ui.label(
                        RichText::new(format!(
                            "Очистка: {done_categories} из {total_categories} · {current} · {}",
                            format::elapsed(started.elapsed())
                        ))
                        .font(font(10.0))
                        .color(TEXT_MUTED),
                    );
                    ui.add_space(2.0);

                    let mut progress = format!(
                        "Удалено {} · {}",
                        format::bytes(*bytes),
                        format::files(*files)
                    );
                    if *errors > 0 {
                        progress.push_str(&format!(" · ошибок: {errors}"));
                    }
                    ui.label(
                        RichText::new(progress)
                            .font(font(10.0))
                            .color(if *errors > 0 { WARNING } else { TEXT_MUTED }),
                    );
                }
            }

            ui.add_space(10.0);

            if !cleanup.is_cleaning() {
                if cleanup.selected_has_extra_confirmation() {
                    widgets::inline_hint(
                        ui,
                        ph::WARNING,
                        "Для «Загрузок», корзины и карантина потребуется отдельное подтверждение.",
                        WARNING,
                    );
                }

                if cleanup.selected_has_max_only() && !settings.max_cleanup {
                    widgets::inline_hint(
                        ui,
                        ph::WARNING,
                        "«Windows.old» и «Загруженные обновления» потребуют отдельного подтверждения.",
                        WARNING,
                    );
                }

                if settings.max_cleanup {
                    widgets::inline_hint(
                        ui,
                        ph::WARNING,
                        "Максимальная чистка выбирает все разделы, включая «Загрузки», корзину, Windows.old и обновления.",
                        WARNING,
                    );
                }

                if !cleanup.results.is_empty()
                    && !cleanup.can_clean()
                    && !cleanup.is_scanning()
                    && !cleanup.selected.is_empty()
                {
                    widgets::inline_hint(
                        ui,
                        ph::INFO,
                        "Выбор изменился. Запустите сканирование заново, чтобы очистить новые разделы.",
                        TEXT_MUTED,
                    );
                }

                widgets::inline_hint(
                    ui,
                    ph::SHIELD_CHECK,
                    "Перед удалением покажем список и запросим подтверждение.",
                    TEXT_DIM,
                );
            }
        });
}

fn status_line(ui: &mut Ui, cleanup: &CleanupState) {
    match &cleanup.clean {
        CleanState::Cleaning {
            done_categories,
            total_categories,
            ..
        } => {
            ui.label(
                RichText::new(format!(
                    "Идёт очистка · {done_categories} из {total_categories}"
                ))
                .font(font(10.5))
                .color(ACCENT),
            );
        }
        CleanState::Done {
            bytes,
            files,
            errors,
            elapsed,
            cancelled,
        } => {
            let mut text = if *cancelled {
                format!(
                    "Очистка остановлена · удалено {} · {} · {}",
                    format::bytes(*bytes),
                    format::files(*files),
                    format::elapsed(*elapsed)
                )
            } else {
                format!(
                    "Очищено {} · {} · {}",
                    format::bytes(*bytes),
                    format::files(*files),
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
        CleanState::Failed(message) => {
            ui.label(RichText::new(message).font(font(10.5)).color(ACCENT));
        }
        CleanState::Idle => match &cleanup.scan {
            ScanState::Idle => {
                ui.label(
                    RichText::new("Сканирование ещё не запускалось")
                        .font(font(10.5))
                        .color(TEXT_DIM),
                );
            }
            ScanState::Scanning {
                done,
                total,
                started,
                ..
            } => {
                ui.label(
                    RichText::new(format!(
                        "Идёт сканирование · {done} из {total} · {}",
                        format::elapsed(started.elapsed())
                    ))
                    .font(font(10.5))
                    .color(TEXT_MUTED),
                );
            }
            ScanState::Failed(message) => {
                ui.label(RichText::new(message).font(font(10.5)).color(ACCENT));
            }
            ScanState::Done {
                bytes,
                files,
                errors,
                elapsed,
                cancelled,
            } => {
                let prefix = if *cancelled {
                    "Остановлено"
                } else {
                    "Найдено"
                };
                let mut text = format!(
                    "{prefix} {} · {} · {}",
                    format::bytes(*bytes),
                    format::files(*files),
                    format::elapsed(*elapsed)
                );
                if *errors > 0 {
                    text.push_str(&format!(" · {errors} недоступно"));
                }

                ui.label(RichText::new(text).font(font(10.5)).color(if *cancelled {
                    WARNING
                } else {
                    TEXT_MUTED
                }));
            }
        },
    }
}

fn group_header(
    ui: &mut Ui,
    group: Group,
    categories: &[CleanupCategory],
    selected: &BTreeSet<CategoryId>,
) {
    let total = categories
        .iter()
        .filter(|category| category.group == group)
        .count();
    let chosen = categories
        .iter()
        .filter(|category| category.group == group && selected.contains(&category.id))
        .count();

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(group_icon(group))
                .font(FontId::new(15.0, FontFamily::Proportional))
                .color(widgets::group_color(group)),
        );
        ui.label(
            RichText::new(group_title(group))
                .font(font_semibold(12.5))
                .color(TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{chosen}/{total}"))
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
        });
    });

    ui.label(
        RichText::new(group_hint(group))
            .font(font(9.5))
            .color(TEXT_DIM),
    );
}

fn group_title(group: Group) -> &'static str {
    match group {
        Group::System => "Система и обновления",
        Group::Browsers => "Браузеры и GPU",
        Group::Personal => "Личные данные",
    }
}

fn group_hint(group: Group) -> &'static str {
    match group {
        Group::System => "Временные данные системы и журналы обслуживания",
        Group::Browsers => "Кэши браузеров и графических драйверов",
        Group::Personal => "Требуют отдельного подтверждения перед удалением",
    }
}

fn group_icon(group: Group) -> &'static str {
    match group {
        Group::System => ph::GEAR_SIX,
        Group::Browsers => ph::BROWSERS,
        Group::Personal => ph::USER,
    }
}

fn confirmation_modal(ctx: &egui::Context, cleanup: &mut CleanupState, downloads_days: u32) {
    let Some(stage) = cleanup.pending_cleanup else {
        return;
    };

    let categories = cleanup.cleanable_categories();
    let sensitive_categories: Vec<_> = categories
        .iter()
        .filter(|category| category.extra_confirmation)
        .cloned()
        .collect();
    let advanced_categories: Vec<_> = categories
        .iter()
        .filter(|category| category.max_only)
        .cloned()
        .collect();
    let (total_bytes, total_files, total_errors) = cleanup.cleanable_totals();
    let has_sensitive = !sensitive_categories.is_empty();
    let has_advanced = !advanced_categories.is_empty();

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Action {
        None,
        Cancel,
        Next,
        Back,
        Start,
    }

    let mut action = Action::None;

    let modal = egui::Modal::new(egui::Id::new("chipclean-cleanup-confirm")).show(ctx, |ui| {
        ui.set_min_width(560.0);
        ui.set_max_width(640.0);

        match stage {
            ConfirmStage::Review => {
                modal_header(
                    ui,
                    ph::TRASH,
                    "Подтвердите очистку",
                    "Проверьте разделы и объём. Удаление нельзя отменить.",
                    ACCENT,
                );

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    modal_stat(ui, "Объём", &format::bytes(total_bytes), TEXT);
                    ui.add_space(24.0);
                    modal_stat(ui, "Файлов/объектов", &format::files(total_files), TEXT);
                    if total_errors > 0 {
                        ui.add_space(24.0);
                        modal_stat(ui, "Недоступно", &total_errors.to_string(), WARNING);
                    }
                });

                ui.add_space(12.0);
                Frame::new()
                    .fill(BG)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("cleanup-confirm-list")
                            .max_height(220.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for category in &categories {
                                    let outcome = cleanup
                                        .results
                                        .get(&category.id)
                                        .copied()
                                        .unwrap_or_default();
                                    modal_category_row(ui, category, &outcome);
                                }
                            });
                    });

                ui.add_space(10.0);

                if total_errors > 0 {
                    warning_box(
                        ui,
                        WARNING,
                        &format!(
                            "{total_errors} объектов были недоступны при сканировании и не будут удалены."
                        ),
                    );
                }

                if has_advanced {
                    warning_box(
                        ui,
                        WARNING,
                        "В выборке есть расширенные разделы: загруженные обновления и Windows.old. Для них потребуется отдельное подтверждение.",
                    );
                }

                if has_sensitive {
                    warning_box(
                        ui,
                        WARNING,
                        "Для особых разделов потребуется отдельный шаг подтверждения.",
                    );
                }

                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Правила разделов применяются заново: новые временные файлы также могут попасть под очистку.",
                    )
                    .font(font(8.5))
                    .color(TEXT_DIM),
                );

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let needs_ack = has_sensitive || has_advanced;
                            let label = if needs_ack { "Продолжить" } else { "Очистить" };
                            if widgets::primary_button(ui, label, ph::TRASH, true).clicked() {
                                action = if needs_ack {
                                    Action::Next
                                } else {
                                    Action::Start
                                };
                            }
                            ui.add_space(8.0);
                            if widgets::secondary_button(ui, "Отмена").clicked() {
                                action = Action::Cancel;
                            }
                        },
                    );
                });
            }
            ConfirmStage::Sensitive => {
                modal_header(
                    ui,
                    ph::WARNING,
                    "Особые разделы",
                    "Эти разделы требуют дополнительного подтверждения.",
                    ACCENT,
                );

                let mut notes = Vec::new();
                if sensitive_categories
                    .iter()
                    .any(|category| category.id == CategoryId::Downloads)
                {
                    notes.push(format!(
                        "Из «Загрузок» будут удалены только файлы старше {downloads_days} дн. на верхнем уровне."
                    ));
                }
                if sensitive_categories
                    .iter()
                    .any(|category| category.id == CategoryId::RecycleBin)
                {
                    notes.push("Корзина будет очищена полностью.".to_owned());
                }
                if sensitive_categories
                    .iter()
                    .any(|category| category.id == CategoryId::Quarantine)
                {
                    notes.push(
                        "Карантин ChipClean будет очищен безвозвратно; восстановить объекты будет нельзя."
                            .to_owned(),
                    );
                }

                ui.add_space(14.0);
                ui.label(
                    RichText::new(notes.join(" "))
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                );

                ui.add_space(12.0);
                Frame::new()
                    .fill(BG)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        for category in &sensitive_categories {
                            let outcome = cleanup
                                .results
                                .get(&category.id)
                                .copied()
                                .unwrap_or_default();
                            modal_category_row(ui, category, &outcome);
                        }
                    });

                ui.add_space(10.0);
                warning_box(
                    ui,
                    ACCENT,
                    "Это действие нельзя отменить. Файлы удаляются безвозвратно и не попадут в корзину.",
                );

                ui.add_space(10.0);
                widgets::confirmation_checkbox(
                    ui,
                    &mut cleanup.sensitive_ack,
                    "Я понимаю и подтверждаю удаление этих файлов",
                );

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let label = if has_advanced {
                                "Продолжить"
                            } else {
                                "Удалить"
                            };
                            if widgets::primary_button(
                                ui,
                                label,
                                ph::TRASH,
                                cleanup.sensitive_ack,
                            )
                            .clicked()
                                && cleanup.sensitive_ack
                            {
                                action = if has_advanced {
                                    Action::Next
                                } else {
                                    Action::Start
                                };
                            }
                            ui.add_space(8.0);
                            if widgets::secondary_button(ui, "Назад").clicked() {
                                action = Action::Back;
                            }
                        },
                    );
                });
            }
            ConfirmStage::Advanced => {
                modal_header(
                    ui,
                    ph::WARNING,
                    "Расширенные разделы",
                    "Windows.old и загруженные обновления удаляются без возможности отката.",
                    WARNING,
                );

                let mut notes = Vec::new();
                if advanced_categories
                    .iter()
                    .any(|category| category.id == CategoryId::WindowsOld)
                {
                    notes.push(
                        "Windows.old хранит предыдущую версию системы: после удаления откат Windows станет невозможен."
                            .to_owned(),
                    );
                }
                if advanced_categories
                    .iter()
                    .any(|category| category.id == CategoryId::UpdateCache)
                {
                    notes.push(
                        "Загруженные обновления будут скачаны заново при следующей проверке."
                            .to_owned(),
                    );
                }

                ui.add_space(14.0);
                ui.label(
                    RichText::new(notes.join(" "))
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                );

                ui.add_space(12.0);
                Frame::new()
                    .fill(BG)
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        for category in &advanced_categories {
                            let outcome = cleanup
                                .results
                                .get(&category.id)
                                .copied()
                                .unwrap_or_default();
                            modal_category_row(ui, category, &outcome);
                        }
                    });

                ui.add_space(10.0);
                warning_box(
                    ui,
                    ACCENT,
                    "Это действие нельзя отменить. Файлы удаляются безвозвратно и не попадут в корзину.",
                );

                ui.add_space(10.0);
                widgets::confirmation_checkbox(
                    ui,
                    &mut cleanup.advanced_ack,
                    "Я понимаю, что откат Windows и повторная загрузка обновлений будут недоступны",
                );

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if widgets::primary_button(
                                ui,
                                "Удалить",
                                ph::TRASH,
                                cleanup.advanced_ack,
                            )
                            .clicked()
                                && cleanup.advanced_ack
                            {
                                action = Action::Start;
                            }
                            ui.add_space(8.0);
                            if widgets::secondary_button(ui, "Назад").clicked() {
                                action = Action::Back;
                            }
                        },
                    );
                });
            }
        }
    });

    if action == Action::None && modal.should_close() {
        cleanup.cancel_cleanup_confirmation();
        return;
    }

    match action {
        Action::None => {}
        Action::Cancel => cleanup.cancel_cleanup_confirmation(),
        Action::Back => match stage {
            ConfirmStage::Review => {}
            ConfirmStage::Sensitive => {
                cleanup.pending_cleanup = Some(ConfirmStage::Review);
                cleanup.sensitive_ack = false;
                cleanup.advanced_ack = false;
            }
            ConfirmStage::Advanced => {
                cleanup.pending_cleanup = Some(if has_sensitive {
                    ConfirmStage::Sensitive
                } else {
                    ConfirmStage::Review
                });
                cleanup.advanced_ack = false;
                if !has_sensitive {
                    cleanup.sensitive_ack = false;
                }
            }
        },
        Action::Next => match stage {
            ConfirmStage::Review => {
                cleanup.pending_cleanup = Some(if has_sensitive {
                    ConfirmStage::Sensitive
                } else {
                    ConfirmStage::Advanced
                });
                cleanup.sensitive_ack = false;
                cleanup.advanced_ack = false;
            }
            ConfirmStage::Sensitive => {
                cleanup.pending_cleanup = Some(ConfirmStage::Advanced);
                cleanup.advanced_ack = false;
            }
            ConfirmStage::Advanced => {}
        },
        Action::Start => {
            if let Err(err) =
                cleanup.start_clean(downloads_days, cleanup.sensitive_ack, cleanup.advanced_ack)
            {
                cleanup.pending_cleanup = None;
                cleanup.clean = CleanState::Failed(err);
            }
        }
    }
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

fn modal_category_row(ui: &mut Ui, category: &CleanupCategory, outcome: &ScanOutcome) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
    let left = rect.left() + 10.0;

    ui.painter().text(
        egui::pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        category.icon,
        FontId::new(16.0, FontFamily::Proportional),
        widgets::group_color(category.group),
    );

    let title_rect = ui.painter().text(
        egui::pos2(left + 26.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        category.title,
        font_semibold(11.0),
        TEXT,
    );

    if category.extra_confirmation {
        ui.painter().text(
            egui::pos2(title_rect.right() + 5.0, title_rect.center().y),
            Align2::LEFT_CENTER,
            ph::WARNING,
            FontId::new(11.0, FontFamily::Proportional),
            WARNING,
        );
    }

    ui.painter().text(
        egui::pos2(left + 26.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        category.summary,
        font(9.0),
        TEXT_DIM,
    );

    let right = rect.right() - 10.0;
    ui.painter().text(
        egui::pos2(right, rect.center().y - 7.0),
        Align2::RIGHT_CENTER,
        format::bytes(outcome.bytes),
        font_semibold(10.5),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(right, rect.center().y + 8.0),
        Align2::RIGHT_CENTER,
        count_label(category.id, outcome.files),
        font(8.5),
        TEXT_DIM,
    );

    ui.add_space(4.0);
}

fn count_label(category: CategoryId, count: u64) -> String {
    if category == CategoryId::RecycleBin {
        format!("{count} объектов")
    } else {
        format::files(count)
    }
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
