use std::collections::BTreeSet;

use egui::{CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Stroke, Ui};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config::Settings,
    core::{
        model::{CategoryId, CleanupCategory, Group},
        state::{CleanupState, ScanState},
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
                        let response = widgets::category_row(column, category, checked, outcome);

                        if response.clicked() {
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
                    ui.label(RichText::new(heading).font(font_semibold(15.0)).color(TEXT));
                    ui.add_space(2.0);
                    status_line(ui, cleanup);
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let scanning = cleanup.is_scanning();
                    let enabled = scanning || cleanup.selected_count() > 0;
                    let label = if scanning {
                        if cleanup.cancel_requested {
                            "Останавливаем…"
                        } else {
                            "Остановить"
                        }
                    } else {
                        "Сканировать"
                    };
                    let icon = if scanning {
                        ph::X
                    } else {
                        ph::MAGNIFYING_GLASS
                    };

                    let response = widgets::primary_button(ui, label, icon, enabled);
                    if response.clicked() && enabled {
                        if scanning {
                            cleanup.cancel_scan();
                        } else {
                            cleanup.start_scan(settings.downloads_older_than_days);
                        }
                    }

                    ui.add_space(14.0);
                    let mut max_cleanup = settings.max_cleanup;
                    if widgets::switch(ui, &mut max_cleanup, "Максимальная чистка")
                    {
                        settings.max_cleanup = max_cleanup;
                        cleanup.set_max_cleanup(max_cleanup);
                    }
                });
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
            }

            ui.add_space(10.0);

            if cleanup.selected_has_extra_confirmation() {
                widgets::inline_hint(
                    ui,
                    ph::WARNING,
                    "Для «Загрузок» и корзины будет запрошено отдельное подтверждение.",
                    WARNING,
                );
            }

            if settings.max_cleanup {
                widgets::inline_hint(
                    ui,
                    ph::WARNING,
                    "Максимальная чистка включает расширенные разделы. Проверьте выбор.",
                    WARNING,
                );
            }

            widgets::inline_hint(
                ui,
                ph::SHIELD_CHECK,
                "Сейчас доступен только скан. Удаление включим после проверки правил безопасности.",
                TEXT_DIM,
            );
        });
}

fn status_line(ui: &mut Ui, cleanup: &CleanupState) {
    match &cleanup.scan {
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
