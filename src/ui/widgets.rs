use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Margin, Rect, Response,
    RichText, Sense, Ui, Vec2,
};
use egui_phosphor::regular as ph;

use crate::{
    core::{
        model::{CleanupCategory, Group, Risk},
        scanner::ScanOutcome,
    },
    theme::*,
    ui::format,
};

pub fn nav_item(ui: &mut Ui, icon: &str, label: &str, selected: bool) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());

    let fill = if selected {
        ACCENT_SOFT
    } else if response.hovered() {
        SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);

    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min + Vec2::new(0.0, 10.0), Vec2::new(3.0, 20.0)),
            CornerRadius::same(2),
            ACCENT,
        );
    }

    let foreground = if selected { TEXT } else { TEXT_MUTED };
    ui.painter().text(
        rect.left_center() + Vec2::new(16.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        FontId::new(17.0, FontFamily::Proportional),
        if selected { ACCENT } else { TEXT_DIM },
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(44.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        if selected {
            font_semibold(13.0)
        } else {
            font(13.0)
        },
        foreground,
    );

    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn category_row(
    ui: &mut Ui,
    category: &CleanupCategory,
    checked: bool,
    outcome: Option<&ScanOutcome>,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), SURFACE_2);
    }

    draw_checkbox(
        ui,
        rect.left_center() + Vec2::new(20.0, 0.0),
        checked,
        false,
    );

    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, 0.0),
        Align2::LEFT_CENTER,
        category.icon,
        FontId::new(19.0, FontFamily::Proportional),
        group_color(category.group),
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(72.0, -9.0),
        Align2::LEFT_CENTER,
        category.title,
        font_semibold(12.0),
        TEXT,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(72.0, 9.0),
        Align2::LEFT_CENTER,
        category.summary,
        font(10.0),
        TEXT_DIM,
    );

    let mut right = rect.right() - 12.0;

    if category.extra_confirmation {
        ui.painter().text(
            egui::pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            ph::WARNING,
            FontId::new(13.0, FontFamily::Proportional),
            WARNING,
        );
        right -= 20.0;
    }

    if let Some(outcome) = outcome {
        ui.painter().text(
            egui::pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            format::bytes(outcome.bytes),
            font_semibold(10.5),
            if outcome.bytes > 0 { TEXT } else { TEXT_DIM },
        );
    } else if category.max_only {
        ui.painter().text(
            egui::pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            "max",
            font(9.0),
            TEXT_DIM,
        );
    }

    let tooltip = format!(
        "{}\n{}\n{}\nПрава администратора: {}",
        category.title,
        category.summary,
        risk_label(category.risk),
        if category.requires_admin {
            "да"
        } else {
            "нет"
        }
    );

    response
        .on_hover_text(tooltip)
        .on_hover_cursor(CursorIcon::PointingHand)
}

fn draw_checkbox(ui: &mut Ui, center: egui::Pos2, checked: bool, muted: bool) {
    let rect = Rect::from_center_size(center, Vec2::splat(18.0));
    let outer = if muted { BORDER } else { ACCENT };

    if checked {
        ui.painter().rect_filled(rect, CornerRadius::same(5), outer);
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            ph::CHECK,
            FontId::new(11.0, FontFamily::Proportional),
            Color32::WHITE,
        );
    } else {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), BORDER);
        ui.painter()
            .rect_filled(rect.shrink(1.0), CornerRadius::same(4), BG);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn selectable_row(
    ui: &mut Ui,
    icon: &str,
    title: &str,
    summary: &str,
    checked: bool,
    enabled: bool,
    accent: Color32,
    right_text: Option<&str>,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 54.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), SURFACE_2);
    }

    draw_checkbox(
        ui,
        rect.left_center() + Vec2::new(20.0, 0.0),
        checked,
        !enabled,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(46.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        FontId::new(19.0, FontFamily::Proportional),
        accent,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(72.0, -9.0),
        Align2::LEFT_CENTER,
        title,
        font_semibold(12.0),
        TEXT,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(72.0, 9.0),
        Align2::LEFT_CENTER,
        summary,
        font(10.0),
        TEXT_DIM,
    );

    if let Some(right_text) = right_text {
        ui.painter().text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            right_text,
            font_semibold(10.5),
            TEXT_MUTED,
        );
    }

    let response = response.on_hover_text(format!("{title}\n{summary}"));

    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn confirmation_checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), SURFACE_2);
    }

    draw_checkbox(
        ui,
        rect.left_center() + Vec2::new(12.0, 0.0),
        *checked,
        false,
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(32.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        font(10.5),
        if *checked { TEXT } else { TEXT_MUTED },
    );

    if response.clicked() {
        *checked = !*checked;
        true
    } else {
        false
    }
}

pub fn primary_button(ui: &mut Ui, label: &str, icon: &str, enabled: bool) -> Response {
    let width = 172.0_f32.min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, 38.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let fill = if !enabled {
        SURFACE_2
    } else if response.is_pointer_button_down_on() {
        ACCENT_DARK
    } else if response.hovered() {
        ACCENT_HOVER
    } else {
        ACCENT
    };
    let foreground = if enabled { Color32::WHITE } else { TEXT_DIM };

    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{icon}  {label}"),
        font_semibold(12.5),
        foreground,
    );

    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    let width = 160.0_f32.min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::click());
    let fill = if response.hovered() {
        SURFACE_3
    } else {
        SURFACE_2
    };

    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font(11.5),
        TEXT,
    );

    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn secondary_button_icon(ui: &mut Ui, label: &str, icon: &str) -> Response {
    let width = 160.0_f32.min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 38.0), Sense::click());
    let fill = if response.hovered() {
        SURFACE_3
    } else {
        SURFACE_2
    };

    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{icon}  {label}"),
        font_semibold(12.0),
        TEXT,
    );

    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn switch(ui: &mut Ui, value: &mut bool, label: &str) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(RichText::new(label).font(font(11.0)).color(TEXT_MUTED));
        ui.add_space(2.0);

        let (rect, response) = ui.allocate_exact_size(Vec2::new(38.0, 21.0), Sense::click());
        let t = ui.ctx().animate_bool(response.id, *value);
        let background = if *value {
            ACCENT
        } else {
            Color32::from_rgb(72, 72, 82)
        };

        ui.painter()
            .rect_filled(rect, CornerRadius::same(11), background);
        let knob_x = rect.left() + 11.0 + (rect.width() - 22.0) * t;
        ui.painter()
            .circle_filled(egui::pos2(knob_x, rect.center().y), 7.5, Color32::WHITE);

        if response.clicked() {
            *value = !*value;
            changed = true;
        }

        response.on_hover_cursor(CursorIcon::PointingHand);
    });

    changed
}

pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 26);
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(font(9.5)).color(color));
        });
}

pub fn progress(ui: &mut Ui, fraction: f32, height: f32, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = CornerRadius::same((height / 2.0).clamp(0.0, 255.0) as u8);

    ui.painter().rect_filled(rect, radius, SURFACE_3);

    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let filled =
            Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
        ui.painter().rect_filled(filled, radius, color);
    }
}

pub fn line(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, BORDER);
}

pub fn inline_hint(ui: &mut Ui, icon: &str, text: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icon)
                .font(FontId::new(13.0, FontFamily::Proportional))
                .color(color),
        );
        ui.label(RichText::new(text).font(font(10.5)).color(color));
    });
}

pub fn group_color(group: Group) -> Color32 {
    match group {
        Group::System => ACCENT,
        Group::Browsers => WARNING,
        Group::Personal => Color32::from_rgb(126, 146, 184),
    }
}

pub fn risk_label(risk: Risk) -> &'static str {
    match risk {
        Risk::Safe => "Безопасно",
        Risk::Caution => "Осторожно",
        Risk::Advanced => "Расширенно",
    }
}

pub fn risk_color(risk: Risk) -> Color32 {
    match risk {
        Risk::Safe => SUCCESS,
        Risk::Caution => WARNING,
        Risk::Advanced => ACCENT,
    }
}
