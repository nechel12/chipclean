use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    config,
    core::{
        services::ServiceEntry,
        startup::{StartupEntry, StartupEntryKind, StartupSource},
        startup_state::{StartupState, StartupTab},
        tasks::TaskEntry,
    },
    theme::*,
    ui::{common, format, widgets},
};

pub fn show(ui: &mut Ui, state: &mut StartupState) {
    state.ensure_loaded();
    common::page_header(ui, Page::Startup);

    info_card(ui);
    ui.add_space(14.0);
    tabs(ui, state);
    ui.add_space(10.0);
    summary(ui, state);
    ui.add_space(14.0);
    list(ui, state);

    if state.pending.is_some() {
        let ctx = ui.ctx().clone();
        confirmation_modal(&ctx, state);
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
                    ph::ROCKET_LAUNCH,
                    FontId::new(22.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Автозагрузка Windows")
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Run/RunOnce, папки «Автозагрузка», задачи планировщика и службы. Отключение обратимо: реестр — .reg-бэкап, файлы — карантин, задачи и службы — переключатель ChipClean.",
                        )
                        .font(font(10.5))
                        .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        widgets::badge(ui, "3 источника", SUCCESS);
                    },
                );
            });
        });
}

fn tabs(ui: &mut Ui, state: &mut StartupState) {
    let mut selected = None;
    let width = ((ui.available_width() - 16.0) / 3.0).clamp(120.0, 190.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (tab, label, icon) in [
            (StartupTab::Run, "Автозагрузка", ph::ROCKET_LAUNCH),
            (StartupTab::Tasks, "Планировщик", ph::CALENDAR_CHECK),
            (StartupTab::Services, "Службы", ph::GEAR_SIX),
        ] {
            let count = state.total_count(tab);
            if tab_button(ui, width, icon, label, count, state.tab == tab) {
                selected = Some(tab);
            }
        }
    });

    if let Some(tab) = selected {
        state.set_tab(tab);
    }
}

fn tab_button(
    ui: &mut Ui,
    width: f32,
    icon: &str,
    label: &str,
    count: usize,
    selected: bool,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::click());
    let fill = if selected {
        ACCENT_SOFT
    } else if response.hovered() {
        SURFACE_3
    } else {
        SURFACE_2
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    if selected {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.0, ACCENT),
            egui::StrokeKind::Inside,
        );
    }

    ui.painter().text(
        rect.left_center() + Vec2::new(12.0, 0.0),
        Align2::LEFT_CENTER,
        format!("{icon}  {label}"),
        if selected {
            font_semibold(11.0)
        } else {
            font(11.0)
        },
        if selected { TEXT } else { TEXT_MUTED },
    );
    ui.painter().text(
        rect.right_center() - Vec2::new(12.0, 0.0),
        Align2::RIGHT_CENTER,
        count.to_string(),
        font_semibold(10.0),
        if selected { ACCENT } else { TEXT_DIM },
    );

    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    widgets::focus_ring(ui, &response, rect, 8);
    response.clicked()
}

fn summary(ui: &mut Ui, state: &mut StartupState) {
    let (active, disabled) = state.counts(state.tab);
    let total = state.total_count(state.tab);

    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(summary_title(state.tab, total))
                            .font(font_semibold(15.0))
                            .color(TEXT),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!("Активно: {active} · Отключено: {disabled}"))
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
                    if state.tab == StartupTab::Run {
                        ui.add_space(8.0);
                        if widgets::secondary_button(ui, "Открыть бэкапы").clicked()
                            && let Err(err) = config::open_dir(&config::backups_dir())
                        {
                            tracing::warn!(%err, "failed to open backups directory");
                        }
                    }
                });
            });

            ui.add_space(10.0);
            widgets::inline_hint(ui, ph::SHIELD_CHECK, hint(state.tab), TEXT_DIM);
        });
}

fn summary_title(tab: StartupTab, total: usize) -> String {
    match tab {
        StartupTab::Run => {
            if total == 0 {
                "Автозагрузка пуста".to_owned()
            } else {
                format!("Найдено {}", format::records(total as u64))
            }
        }
        StartupTab::Tasks => {
            if total == 0 {
                "Задачи не найдены".to_owned()
            } else {
                format!("Задач: {total}")
            }
        }
        StartupTab::Services => {
            if total == 0 {
                "Службы не найдены".to_owned()
            } else {
                format!("Служб: {total}")
            }
        }
    }
}

fn hint(tab: StartupTab) -> &'static str {
    match tab {
        StartupTab::Run => {
            "Каждое изменение подтверждается; отключение можно отменить этим же переключателем. Для реестра создаётся .reg-бэкап."
        }
        StartupTab::Tasks => {
            "Отключение задачи не останавливает уже запущенный экземпляр. Задача не удаляется."
        }
        StartupTab::Services => {
            "Меняется только тип запуска; работающая служба продолжит работу до перезагрузки. Службы, отключённые вне ChipClean, отмечены предупреждением и переводятся только в ручной запуск."
        }
    }
}

fn list(ui: &mut Ui, state: &mut StartupState) {
    let total = state.total_count(state.tab);
    if total == 0 {
        empty_state(ui, state.tab);
        return;
    }

    let mut toggle_index = None;

    egui::ScrollArea::vertical()
        .id_salt("startup-entries")
        .auto_shrink([false, false])
        .show(ui, |ui| match state.tab {
            StartupTab::Run => {
                for (index, entry) in state.entries.iter().enumerate() {
                    if startup_row(ui, entry, index) {
                        toggle_index = Some(index);
                    }
                    ui.add_space(4.0);
                }
            }
            StartupTab::Tasks => {
                for (index, entry) in state.tasks.iter().enumerate() {
                    if task_row(ui, entry, index) {
                        toggle_index = Some(index);
                    }
                    ui.add_space(4.0);
                }
            }
            StartupTab::Services => {
                for (index, entry) in state.services.iter().enumerate() {
                    if service_row(ui, entry, index) {
                        toggle_index = Some(index);
                    }
                    ui.add_space(4.0);
                }
            }
        });

    if let Some(index) = toggle_index {
        state.request_toggle(index);
    }
}

fn confirmation_modal(ctx: &egui::Context, state: &mut StartupState) {
    let Some(pending) = state.pending.clone() else {
        return;
    };
    let mut action = ModalAction::None;
    let run = pending.tab == StartupTab::Run;
    let service = pending.tab == StartupTab::Services;
    let color = if pending.enabling && !pending.manual_restore {
        SUCCESS
    } else {
        WARNING
    };
    let icon = if pending.manual_restore || service {
        ph::GEAR_SIX
    } else if run {
        ph::ROCKET_LAUNCH
    } else {
        ph::CALENDAR_CHECK
    };
    let title = if pending.manual_restore {
        "Вернуть службу в ручной запуск?"
    } else if pending.enabling {
        match pending.tab {
            StartupTab::Run => "Включить автозагрузку?",
            StartupTab::Tasks => "Включить задачу?",
            StartupTab::Services => "Включить службу?",
        }
    } else {
        match pending.tab {
            StartupTab::Run => "Отключить автозагрузку?",
            StartupTab::Tasks => "Отключить задачу?",
            StartupTab::Services => "Отключить службу?",
        }
    };
    let subtitle = if pending.manual_restore {
        "Служба была отключена вне ChipClean; автозапуск не включится."
    } else if pending.enabling {
        "Автозапуск будет восстановлен."
    } else {
        "Автозапуск будет отключён; отмена доступна этим же переключателем."
    };

    let modal = egui::Modal::new(egui::Id::new("startup-confirm"))
        .frame(
            Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(20)),
        )
        .show(ctx, |ui| {
        ui.set_min_width(540.0);
        ui.set_max_width(620.0);

        modal_header(ui, icon, title, subtitle, color);

        ui.add_space(14.0);
        Frame::new()
            .fill(BG)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(&pending.title)
                        .font(font_semibold(12.0))
                        .color(TEXT),
                );
                ui.add_space(3.0);
                ui.label(
                    RichText::new(&pending.command)
                        .font(FontId::new(9.0, FontFamily::Monospace))
                        .color(TEXT_DIM),
                );
                if pending.system {
                    ui.add_space(5.0);
                    ui.label(
                        RichText::new("Системный компонент Windows")
                            .font(font(9.5))
                            .color(WARNING),
                    );
                }
            });

        ui.add_space(10.0);
        let warning = if pending.manual_restore {
            "Служба останется остановленной. Тип запуска станет «Вручную»: её сможет запустить Windows или приложение, но автоматически при старте системы она запускаться не будет."
        } else if service {
            "Служба может быть нужна Windows или другому приложению. Работающая служба продолжит работу до остановки или перезагрузки."
        } else if run {
            "Запись может запускать обновления или фоновые компоненты при входе в систему. Для реестра создаётся .reg-бэкап, файл папки перемещается в карантин."
        } else {
            "Задача может выполнять обновления или обслуживание. Уже запущенный экземпляр не останавливается."
        };
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(255, 179, 71, 18))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(255, 179, 71, 60),
            ))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(ph::WARNING)
                            .font(FontId::new(15.0, FontFamily::Proportional))
                            .color(WARNING),
                    );
                    ui.label(RichText::new(warning).font(font(10.0)).color(TEXT_MUTED));
                });
            });

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary_button(
                    ui,
                    if pending.manual_restore {
                        "В ручной запуск"
                    } else if pending.enabling {
                        "Включить"
                    } else if run {
                        "Отключить автозагрузку"
                    } else if service {
                        "Отключить службу"
                    } else {
                        "Отключить задачу"
                    },
                    ph::POWER,
                    true,
                )
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
    if action == ModalAction::None
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        action = ModalAction::Confirm;
    }

    match action {
        ModalAction::None => {}
        ModalAction::Cancel => state.cancel_toggle(),
        ModalAction::Confirm => state.confirm_toggle(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModalAction {
    None,
    Cancel,
    Confirm,
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

fn empty_state(ui: &mut Ui, tab: StartupTab) {
    let text = match tab {
        StartupTab::Run => "Записи автозагрузки не найдены.",
        StartupTab::Tasks => "Задачи планировщика не найдены.",
        StartupTab::Services => "Службы не найдены.",
    };

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

fn startup_row(ui: &mut Ui, entry: &StartupEntry, index: usize) -> bool {
    let command = if entry.command.is_empty() {
        "—"
    } else {
        entry.command.as_str()
    };
    toggle_row(
        ui,
        "startup",
        index,
        &entry.title,
        command,
        entry.enabled,
        entry.source.short_label(),
        false,
        entry_icon(entry),
        if entry.enabled {
            source_color(entry.source)
        } else {
            TEXT_DIM
        },
        &format!("{}\n{}", entry.source.label(), entry.command),
    )
}

fn task_row(ui: &mut Ui, entry: &TaskEntry, index: usize) -> bool {
    let command = if entry.command.is_empty() {
        entry.path.as_str()
    } else {
        entry.command.as_str()
    };
    toggle_row(
        ui,
        "tasks",
        index,
        &entry.title,
        command,
        entry.enabled,
        if entry.system {
            "Windows"
        } else {
            "Задача"
        },
        false,
        if entry.enabled {
            ph::CALENDAR_CHECK
        } else {
            ph::PROHIBIT
        },
        if entry.enabled { WARNING } else { TEXT_DIM },
        &format!("{}\n{}\n{}", entry.path, entry.status, entry.command),
    )
}

fn service_row(ui: &mut Ui, entry: &ServiceEntry, index: usize) -> bool {
    let command = format!("{} · {}", entry.status, entry.command);
    let manual_restore = !entry.enabled && !entry.managed;
    let source = if entry.enabled {
        entry.start_label.as_str()
    } else if manual_restore {
        "Вне ChipClean"
    } else {
        "Отключена"
    };
    toggle_row(
        ui,
        "services",
        index,
        &entry.title,
        &command,
        entry.enabled,
        source,
        manual_restore,
        if entry.enabled {
            ph::GEAR_SIX
        } else {
            ph::PROHIBIT
        },
        if entry.enabled {
            ACCENT
        } else if manual_restore {
            WARNING
        } else {
            TEXT_DIM
        },
        &format!(
            "{}\n{} · {}\n{}",
            entry.name, entry.status, entry.start_label, entry.command
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn toggle_row(
    ui: &mut Ui,
    id_salt: &str,
    index: usize,
    title: &str,
    command: &str,
    enabled: bool,
    source: &str,
    manual_restore: bool,
    icon: &str,
    accent: Color32,
    tooltip: &str,
) -> bool {
    let (rect, row_response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::hover());
    if row_response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), SURFACE_2);
    }

    ui.painter().text(
        rect.left_center() + Vec2::new(20.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        FontId::new(19.0, FontFamily::Proportional),
        accent,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, -10.0),
        Align2::LEFT_CENTER,
        shorten(title, 40),
        font_semibold(12.0),
        if enabled { TEXT } else { TEXT_MUTED },
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, 9.0),
        Align2::LEFT_CENTER,
        shorten(if command.is_empty() { "—" } else { command }, 70),
        FontId::new(9.0, FontFamily::Monospace),
        TEXT_DIM,
    );

    let switch_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 27.0, rect.center().y),
        Vec2::new(38.0, 21.0),
    );
    let switch_response = ui.interact(
        switch_rect,
        ui.id().with(("startup-toggle", id_salt, index)),
        Sense::click(),
    );
    let t = ui.ctx().animate_bool(switch_response.id, enabled);
    let background = if enabled {
        ACCENT
    } else {
        Color32::from_rgb(72, 72, 82)
    };
    ui.painter()
        .rect_filled(switch_rect, CornerRadius::same(11), background);
    let knob_x = switch_rect.left() + 11.0 + (switch_rect.width() - 22.0) * t;
    ui.painter().circle_filled(
        egui::pos2(knob_x, switch_rect.center().y),
        7.5,
        Color32::WHITE,
    );

    ui.painter().text(
        egui::pos2(switch_rect.left() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        source,
        font(9.0),
        if enabled || manual_restore {
            accent
        } else {
            TEXT_DIM
        },
    );

    let switch_response = switch_response
        .on_hover_text(if enabled {
            "Отключить автозапуск"
        } else if manual_restore {
            "Вернуть в ручной запуск"
        } else {
            "Включить автозапуск"
        })
        .on_hover_cursor(CursorIcon::PointingHand);

    let _ = row_response.on_hover_text(tooltip);

    switch_response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            enabled,
            format!("{title}. {source}"),
        )
    });
    widgets::focus_ring(ui, &switch_response, switch_rect, 11);

    switch_response.clicked()
}

fn entry_icon(entry: &StartupEntry) -> &'static str {
    if !entry.enabled {
        return ph::PROHIBIT;
    }

    match &entry.kind {
        StartupEntryKind::Registry { .. } => ph::ROCKET_LAUNCH,
        StartupEntryKind::Folder { .. } => ph::FOLDER_SIMPLE,
        StartupEntryKind::Disabled(_) => ph::PROHIBIT,
    }
}

fn source_color(source: StartupSource) -> Color32 {
    match source {
        StartupSource::HkcuRun | StartupSource::HklmRun => ACCENT,
        StartupSource::UserFolder | StartupSource::CommonFolder => WARNING,
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
