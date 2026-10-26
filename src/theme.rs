use std::{env, fs, path::PathBuf};

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};

pub const BG: Color32 = Color32::from_rgb(10, 10, 12);
pub const SIDEBAR: Color32 = Color32::from_rgb(13, 13, 16);
pub const SURFACE: Color32 = Color32::from_rgb(20, 20, 24);
pub const SURFACE_2: Color32 = Color32::from_rgb(26, 26, 31);
pub const SURFACE_3: Color32 = Color32::from_rgb(34, 34, 40);
pub const BORDER: Color32 = Color32::from_rgb(43, 43, 51);
pub const ACCENT: Color32 = Color32::from_rgb(229, 72, 77);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(242, 90, 95);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(184, 52, 57);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(52, 22, 26);
pub const TEXT: Color32 = Color32::from_rgb(236, 236, 240);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(148, 148, 160);
pub const TEXT_DIM: Color32 = Color32::from_rgb(104, 104, 116);
pub const SUCCESS: Color32 = Color32::from_rgb(70, 196, 138);
pub const WARNING: Color32 = Color32::from_rgb(232, 163, 61);

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    load_system_fonts(&mut fonts);
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    attach_semibold_family(&mut fonts);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals();
        style.text_styles = [
            (TextStyle::Heading, font_semibold(26.0)),
            (TextStyle::Body, font(13.5)),
            (TextStyle::Button, font(13.0)),
            (TextStyle::Small, font(11.5)),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style.spacing.interact_size = egui::vec2(38.0, 34.0);
        style.spacing.slider_width = 180.0;
        style.animation_time = 0.12;
    });
}

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.extreme_bg_color = Color32::from_rgb(8, 8, 10);
    visuals.faint_bg_color = SURFACE_2;
    visuals.code_bg_color = SURFACE_2;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = SURFACE_2;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_2;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(9);

    visuals.widgets.hovered.bg_fill = SURFACE_3;
    visuals.widgets.hovered.weak_bg_fill = SURFACE_3;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(9);

    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(9);
    visuals.widgets.open = visuals.widgets.active;

    visuals
}

fn fonts_dir() -> PathBuf {
    env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}

fn load_system_fonts(fonts: &mut FontDefinitions) {
    let dir = fonts_dir();

    if let Ok(bytes) = fs::read(dir.join("segoeui.ttf")) {
        fonts
            .font_data
            .insert("segoe-ui".to_owned(), FontData::from_owned(bytes).into());

        let mut family = vec!["segoe-ui".to_owned()];
        if let Some(defaults) = fonts.families.get(&FontFamily::Proportional) {
            family.extend(defaults.iter().cloned());
        }
        fonts.families.insert(FontFamily::Proportional, family);
    }

    if let Ok(bytes) = fs::read(dir.join("seguisb.ttf")) {
        fonts.font_data.insert(
            "segoe-ui-semibold".to_owned(),
            FontData::from_owned(bytes).into(),
        );
    }
}

fn attach_semibold_family(fonts: &mut FontDefinitions) {
    let proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut family = Vec::new();
    if fonts.font_data.contains_key("segoe-ui-semibold") {
        family.push("segoe-ui-semibold".to_owned());
    }
    family.extend(proportional);

    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), family);
}
