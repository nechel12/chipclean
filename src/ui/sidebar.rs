use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Sense, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    app::{ChipCleanApp, Page},
    theme::*,
    ui::{format, widgets},
};

pub fn show(app: &mut ChipCleanApp, root: &mut Ui) {
    let elevated = app.elevated;
    let disk = app.disk;

    egui::Panel::left("chipclean-sidebar")
        .exact_size(236.0)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(Margin {
            left: 16,
            right: 16,
            top: 20,
            bottom: 16,
        }))
        .show(root, |ui| {
            logo(ui);
            ui.add_space(26.0);
            ui.label(RichText::new("ИНСТРУМЕНТЫ").font(font(9.0)).color(TEXT_DIM));
            ui.add_space(8.0);

            let nav_height = (ui.available_height() - 190.0).max(220.0);
            egui::ScrollArea::vertical()
                .id_salt("sidebar-nav")
                .max_height(nav_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let page = &mut app.page;
                    nav(ui, page, Page::Cleanup, ph::BROOM);
                    nav(ui, page, Page::Registry, ph::DATABASE);
                    nav(ui, page, Page::RegistryEditor, ph::TREE_STRUCTURE);
                    nav(ui, page, Page::Snapshots, ph::CAMERA);
                    nav(ui, page, Page::Privacy, ph::FINGERPRINT);
                    nav(ui, page, Page::Community, ph::USERS_THREE);
                    nav(ui, page, Page::Startup, ph::ROCKET_LAUNCH);
                    nav(ui, page, Page::Traces, ph::FOOTPRINTS);
                    nav(ui, page, Page::Programs, ph::PACKAGE);
                    nav(ui, page, Page::Settings, ph::GEAR_SIX);
                });

            ui.add_space(8.0);
            admin_card(ui, elevated);
            ui.add_space(10.0);
            disk_card(ui, disk);
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!("v{} · Rust", env!("CARGO_PKG_VERSION")))
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
        });
}

fn nav(ui: &mut Ui, page: &mut Page, target: Page, icon: &str) {
    if widgets::nav_item(ui, icon, target.label(), *page == target).clicked() {
        *page = target;
    }
}

fn logo(ui: &mut Ui) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(11), ACCENT);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            ph::SHIELD_CHECK,
            FontId::new(21.0, FontFamily::Proportional),
            Color32::WHITE,
        );

        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.label(
                RichText::new("ChipClean")
                    .font(font_semibold(17.0))
                    .color(TEXT),
            );
            ui.label(
                RichText::new("Windows care")
                    .font(font(9.5))
                    .color(TEXT_DIM),
            );
        });
    });
}

fn admin_card(ui: &mut Ui, elevated: bool) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(
                    rect.center(),
                    4.0,
                    if elevated { SUCCESS } else { WARNING },
                );
                ui.label(
                    RichText::new(if elevated {
                        "Администратор"
                    } else {
                        "Ограниченный режим"
                    })
                    .font(font(10.0))
                    .color(if elevated { TEXT_MUTED } else { WARNING }),
                );
            });
        });
}

fn disk_card(ui: &mut Ui, disk: Option<crate::win::disk::DiskSpace>) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(11))
        .show(ui, |ui| {
            ui.label(
                RichText::new("СИСТЕМНЫЙ ДИСК")
                    .font(font(8.5))
                    .color(TEXT_DIM),
            );
            ui.add_space(2.0);

            if let Some(disk) = disk {
                ui.label(
                    RichText::new(format!("{} свободно", format::bytes(disk.free)))
                        .font(font_semibold(12.0))
                        .color(TEXT),
                );
                ui.add_space(7.0);
                widgets::progress(
                    ui,
                    disk.used_fraction(),
                    4.0,
                    if disk.used_fraction() > 0.9 {
                        ACCENT
                    } else {
                        Color32::from_rgb(120, 92, 96)
                    },
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("из {}", format::bytes(disk.total)))
                        .font(font(9.0))
                        .color(TEXT_DIM),
                );
            } else {
                ui.label(RichText::new("нет данных").font(font(10.0)).color(TEXT_DIM));
            }
        });
}
