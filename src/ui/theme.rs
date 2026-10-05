//! Colors, fonts and egui visuals: black board, neon green signal, gold contacts.

use eframe::egui::{
    self, epaint::Shadow, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle,
};

pub const BG: Color32 = Color32::from_rgb(2, 5, 3);
pub const GREEN: Color32 = Color32::from_rgb(0, 255, 65);
pub const GREEN_SOFT: Color32 = Color32::from_rgb(120, 255, 150);
pub const GREEN_DIM: Color32 = Color32::from_rgb(0, 110, 36);
pub const GREEN_DEEP: Color32 = Color32::from_rgb(0, 46, 16);
pub const GOLD: Color32 = Color32::from_rgb(212, 175, 55);
pub const GOLD_DIM: Color32 = Color32::from_rgb(120, 98, 30);
pub const TEXT: Color32 = Color32::from_rgb(214, 255, 222);
pub const TEXT_DIM: Color32 = Color32::from_rgb(112, 160, 122);
pub const RED: Color32 = Color32::from_rgb(255, 82, 102);

pub const TITLE_FAMILY: &str = "title";

static MONO_BOLD: &[u8] = include_bytes!("../../assets/fonts/DejaVuSansMono-Bold.ttf");

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn panel_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(3, 12, 6, 228)
}

pub fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(TITLE_FAMILY.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("dejavu_mono".to_owned(), FontData::from_static(friendly_converter::pdf::fonts::MONO));
    fonts
        .font_data
        .insert("dejavu_sans".to_owned(), FontData::from_static(friendly_converter::pdf::fonts::SANS));
    fonts.font_data.insert("dejavu_mono_bold".to_owned(), FontData::from_static(MONO_BOLD));

    let mono_family = fonts.families.entry(FontFamily::Monospace).or_default();
    mono_family.insert(0, "dejavu_mono".to_owned());
    mono_family.push("dejavu_sans".to_owned());

    let prop_family = fonts.families.entry(FontFamily::Proportional).or_default();
    prop_family.insert(0, "dejavu_mono".to_owned());
    prop_family.push("dejavu_sans".to_owned());

    fonts.families.insert(
        FontFamily::Name(TITLE_FAMILY.into()),
        vec!["dejavu_mono_bold".to_owned(), "dejavu_mono".to_owned(), "dejavu_sans".to_owned()],
    );
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, mono(20.0)),
        (TextStyle::Body, mono(13.5)),
        (TextStyle::Monospace, mono(13.0)),
        (TextStyle::Button, mono(13.5)),
        (TextStyle::Small, mono(11.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.combo_width = 96.0;

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(4, 14, 7);
    v.window_stroke = Stroke::new(1.0, GREEN_DIM);
    v.window_rounding = Rounding::ZERO;
    v.menu_rounding = Rounding::ZERO;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.extreme_bg_color = Color32::from_rgb(1, 6, 3);
    v.faint_bg_color = Color32::from_rgb(4, 14, 7);
    v.code_bg_color = Color32::from_rgb(4, 14, 7);
    v.hyperlink_color = GREEN;
    v.selection.bg_fill = alpha(GREEN, 70);
    v.selection.stroke = Stroke::new(1.0, GREEN);
    v.warn_fg_color = GOLD;
    v.error_fg_color = RED;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = Color32::from_rgb(4, 14, 7);
    w.noninteractive.weak_bg_fill = Color32::from_rgb(4, 14, 7);
    w.noninteractive.bg_stroke = Stroke::new(1.0, GREEN_DEEP);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    w.noninteractive.rounding = Rounding::ZERO;

    w.inactive.bg_fill = Color32::from_rgb(5, 20, 10);
    w.inactive.weak_bg_fill = Color32::from_rgb(5, 20, 10);
    w.inactive.bg_stroke = Stroke::new(1.0, GREEN_DIM);
    w.inactive.fg_stroke = Stroke::new(1.0, GREEN_SOFT);
    w.inactive.rounding = Rounding::ZERO;

    w.hovered.bg_fill = Color32::from_rgb(8, 38, 17);
    w.hovered.weak_bg_fill = Color32::from_rgb(8, 38, 17);
    w.hovered.bg_stroke = Stroke::new(1.0, GREEN);
    w.hovered.fg_stroke = Stroke::new(1.5, GREEN);
    w.hovered.rounding = Rounding::ZERO;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = Color32::from_rgb(0, 70, 26);
    w.active.weak_bg_fill = Color32::from_rgb(0, 70, 26);
    w.active.bg_stroke = Stroke::new(1.0, GOLD);
    w.active.fg_stroke = Stroke::new(1.5, TEXT);
    w.active.rounding = Rounding::ZERO;
    w.active.expansion = 0.0;

    w.open.bg_fill = Color32::from_rgb(8, 38, 17);
    w.open.weak_bg_fill = Color32::from_rgb(8, 38, 17);
    w.open.bg_stroke = Stroke::new(1.0, GOLD);
    w.open.fg_stroke = Stroke::new(1.0, GREEN);
    w.open.rounding = Rounding::ZERO;

    style.visuals = v;
    ctx.set_style(style);
}
