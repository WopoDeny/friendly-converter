//! Custom neon widgets.

use super::theme::{alpha, mono, GREEN_DEEP, TEXT_DIM};
use eframe::egui::{self, Align2, Color32, CursorIcon, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

/// Gold or green L-shaped brackets on the corners of a rect (HUD frame).
pub fn corners(painter: &Painter, rect: Rect, color: Color32, len: f32) {
    let s = Stroke::new(1.6, color);
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    painter.line_segment([Pos2::new(l, t), Pos2::new(l + len, t)], s);
    painter.line_segment([Pos2::new(l, t), Pos2::new(l, t + len)], s);
    painter.line_segment([Pos2::new(r, t), Pos2::new(r - len, t)], s);
    painter.line_segment([Pos2::new(r, t), Pos2::new(r, t + len)], s);
    painter.line_segment([Pos2::new(l, b), Pos2::new(l + len, b)], s);
    painter.line_segment([Pos2::new(l, b), Pos2::new(l, b - len)], s);
    painter.line_segment([Pos2::new(r, b), Pos2::new(r - len, b)], s);
    painter.line_segment([Pos2::new(r, b), Pos2::new(r, b - len)], s);
}

/// Text with a soft neon halo.
pub fn glow_text(painter: &Painter, pos: Pos2, anchor: Align2, text: &str, font: egui::FontId, color: Color32) -> Rect {
    for (dx, dy, a) in [(-2.0, 0.0, 26u8), (2.0, 0.0, 26), (0.0, -2.0, 26), (0.0, 2.0, 26), (-1.0, -1.0, 40), (1.0, 1.0, 40)] {
        painter.text(pos + Vec2::new(dx, dy), anchor, text, font.clone(), alpha(color, a));
    }
    painter.text(pos, anchor, text, font, color)
}

/// Rectangular neon button. Disabled buttons are dim and ignore clicks.
pub fn neon_button(ui: &mut Ui, text: &str, accent: Color32, enabled: bool) -> Response {
    let font = mono(13.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), accent);
    let size = galley.size() + Vec2::new(26.0, 12.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(size.x, size.y.max(30.0)), sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = enabled && response.hovered();
        let pressed = enabled && response.is_pointer_button_down_on();
        let (fill, stroke, text_color) = if !enabled {
            (alpha(GREEN_DEEP, 90), Stroke::new(1.0, alpha(accent, 50)), alpha(accent, 90))
        } else if pressed {
            (alpha(accent, 90), Stroke::new(1.4, accent), Color32::BLACK)
        } else if hovered {
            (alpha(accent, 38), Stroke::new(1.4, accent), accent)
        } else {
            (Color32::from_rgba_unmultiplied(2, 10, 5, 225), Stroke::new(1.0, alpha(accent, 170)), accent)
        };
        if hovered {
            painter.rect_stroke(rect.expand(3.0), 0.0, Stroke::new(3.0, alpha(accent, 30)));
        }
        painter.rect_filled(rect, 0.0, fill);
        painter.rect_stroke(rect, 0.0, stroke);
        if enabled {
            corners(painter, rect.expand(2.0), alpha(accent, if hovered { 255 } else { 140 }), 5.0);
        }
        painter.text(rect.center(), Align2::CENTER_CENTER, text, font, text_color);
    }
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Small format tag like [PDF].
pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let font = mono(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), color);
    let size = Vec2::new(galley.size().x.max(34.0) + 12.0, 20.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, alpha(color, 28));
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, alpha(color, 160)));
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

/// Section caption with a short leading trace line.
pub fn caption(ui: &mut Ui, text: &str, color: Color32) {
    let font = mono(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), color);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(galley.size().x + 26.0, 18.0), Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.left() + 12.0, y)], Stroke::new(1.4, color));
    painter.circle_filled(Pos2::new(rect.left() + 15.0, y), 2.4, color);
    painter.text(Pos2::new(rect.left() + 22.0, y), Align2::LEFT_CENTER, text, font, color);
}

/// Dim helper text.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_DIM).font(mono(11.5)));
}
