use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui,
    Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::Page,
    core::disk_analysis_state::DiskAnalysisState,
    theme::*,
    ui::{common, format},
};

pub fn show(ui: &mut Ui, state: &mut DiskAnalysisState) {
    common::page_header(ui, Page::DiskAnalysis);

    toolbar(ui, state);

    if let Some(error) = &state.error {
        ui.add_space(10.0);
        Frame::new()
            .fill(ERROR_SOFT)
            .stroke(Stroke::new(1.0, ERROR))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.label(RichText::new(error).font(font(11.0)).color(ERROR_TEXT));
            });
    }

    ui.add_space(12.0);

    summary(ui, state);
    ui.add_space(10.0);

    egui::ScrollArea::vertical()
        .id_salt("disk-analysis-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let files = state.top_files.clone();
            if files.is_empty() {
                ui.label(
                    RichText::new("Запустите сканирование, чтобы увидеть крупнейшие файлы.")
                        .font(font(11.0))
                        .color(TEXT_DIM),
                );
            } else {
                for entry in files.iter().take(150) {
                    file_row(ui, entry);
                    ui.add_space(2.0);
                }
            }
        });
}

fn toolbar(ui: &mut Ui, state: &mut DiskAnalysisState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Том").font(font(11.0)).color(TEXT_MUTED));
        for drive in state.roots.clone() {
            let selected = drive == state.selected_root;
            let label = format!("{} ", drive.trim_end_matches('\\'));
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(&label)
                            .font(font_semibold(11.5))
                            .color(if selected { Color32::WHITE } else { TEXT }),
                    )
                    .fill(if selected { ACCENT } else { SURFACE_2 })
                    .corner_radius(7),
                )
                .clicked()
            {
                state.selected_root = drive;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.is_scanning() {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Отмена").font(font(11.5)).color(TEXT_MUTED),
                        )
                        .corner_radius(8),
                    )
                    .clicked()
                {
                    state.cancel_scan();
                }
            } else if ui
                .add(
                    egui::Button::new(
                        RichText::new("Сканировать")
                            .font(font_semibold(12.0))
                            .color(Color32::WHITE),
                    )
                    .fill(ACCENT)
                    .min_size(Vec2::new(140.0, 34.0))
                    .corner_radius(8),
                )
                .clicked()
            {
                state.start_scan();
            }
        });
    });

    if state.is_scanning() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if state.progress_current.is_empty() {
                    "Сбор данных…".to_owned()
                } else {
                    format!("Собрано файлов: {}", state.progress_collected)
                })
                .font(font(9.5))
                .color(TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !state.progress_current.is_empty() {
                    ui.label(
                        RichText::new(&state.progress_current)
                            .font(font(9.0))
                            .color(TEXT_DIM),
                    );
                }
            });
        });
    }
}

fn summary(ui: &mut Ui, state: &DiskAnalysisState) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(13))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(9), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    ph::HARD_DRIVES,
                    FontId::new(18.0, FontFamily::Proportional),
                    ACCENT,
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    let headline = if state.is_scanning() {
                        "Идёт сканирование…".to_owned()
                    } else if state.total_bytes > 0 {
                        format!(
                            "Просканировано: {} · файлов: {}",
                            format::bytes(state.total_bytes),
                            state.files_count
                        )
                    } else {
                        "Сканирование не запускалось".to_owned()
                    };
                    ui.label(
                        RichText::new(headline)
                            .font(font_semibold(13.0))
                            .color(TEXT),
                    );
                    if state.total_bytes > 0 && !state.is_scanning() {
                        ui.label(
                            RichText::new(format!(
                                "Затрачено: {} с · показано крупнейших: {}",
                                state.elapsed.as_secs(),
                                state.top_files.len().min(150)
                            ))
                            .font(font(9.5))
                            .color(TEXT_MUTED),
                        );
                    }
                });
            });
        });
}

fn file_row(ui: &mut Ui, entry: &crate::core::disk_analysis::DiskFileEntry) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), SURFACE_2);
    }

    let file_name = entry
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.display().to_string());
    let parent = entry
        .path
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();

    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        file_name,
        font(10.5),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 260.0, rect.center().y),
        Align2::LEFT_CENTER,
        parent,
        font(9.0),
        TEXT_DIM,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format::bytes(entry.size),
        font_semibold(10.5),
        TEXT_MUTED,
    );
}
