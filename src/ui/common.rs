use egui::{RichText, Ui};

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
            let version = env!("CARGO_PKG_VERSION");
            widgets::badge(ui, &format!("v{version} · beta"), TEXT_DIM);
        });
    });

    ui.add_space(18.0);
    widgets::line(ui);
    ui.add_space(18.0);
}
