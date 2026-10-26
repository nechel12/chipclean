use egui::{
    Align2, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{app::Page, theme::*, ui::widgets};

pub fn page_header(ui: &mut Ui, page: Page) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(page.title())
                    .font(font_semibold(26.0))
                    .color(TEXT),
            );
            ui.add_space(3.0);
            ui.label(
                RichText::new(page.subtitle())
                    .font(font(11.5))
                    .color(TEXT_MUTED),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widgets::badge(ui, "0.1.0 · каркас", TEXT_DIM);
        });
    });

    ui.add_space(18.0);
    widgets::line(ui);
    ui.add_space(18.0);
}

pub fn empty_state(ui: &mut Ui, icon: &str, title: &str, description: &str, bullets: &[&str]) {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(22))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(12), ACCENT_SOFT);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icon,
                    FontId::new(23.0, FontFamily::Proportional),
                    ACCENT,
                );

                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).font(font_semibold(18.0)).color(TEXT));
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new(description)
                            .font(font(11.5))
                            .color(TEXT_MUTED),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    widgets::badge(ui, "Следующий этап", WARNING);
                });
            });

            ui.add_space(20.0);
            for bullet in bullets {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(ph::CHECK_CIRCLE)
                            .font(FontId::new(13.0, FontFamily::Proportional))
                            .color(SUCCESS),
                    );
                    ui.label(RichText::new(*bullet).font(font(11.0)).color(TEXT_MUTED));
                });
            }
        });
}
