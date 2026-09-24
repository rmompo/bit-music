//! Small shared widgets.

use eframe::egui::{Button, Color32, Painter, Pos2, Rect, Response, RichText, Shape, Stroke, Ui, Vec2, Widget};

/// Side of every icon button, in points. Icon buttons are square and the
/// icon sits in the middle.
pub const ICON_BUTTON_SIZE: f32 = 28.0;
const ICON_SIZE: f32 = 16.0;

/// A square (1:1) button with a centered icon; `selected` draws it pressed
/// (toggles such as loop and mute).
pub struct IconButton<'a> {
    icon: &'a str,
    selected: bool,
    side: f32,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: &'a str) -> Self {
        Self { icon, selected: false, side: ICON_BUTTON_SIZE }
    }

    /// A different side, in points (the icon scales with it).
    pub fn size(mut self, side: f32) -> Self {
        self.side = side;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.add(
            Button::new(RichText::new(self.icon).size(ICON_SIZE * self.side / ICON_BUTTON_SIZE))
                .min_size(Vec2::splat(self.side))
                .selected(self.selected),
        )
    }
}

/// How many oscilloscope points to ask for a width, one per two points of
/// screen (enough detail, cheap to compute).
pub fn scope_points(width: f32) -> usize {
    (width / 2.0).max(2.0) as usize
}

/// Draws an oscilloscope trace across the whole of `rect`: `values` are
/// samples in -1.0..=1.0, evenly spread over the width, centered vertically.
/// Meant as a faint background, so pass a translucent `color`.
pub fn paint_scope(painter: &Painter, rect: Rect, values: &[f32], color: Color32) {
    if values.len() < 2 {
        return;
    }
    let middle = rect.center().y;
    let amplitude = rect.height() * 0.45;
    let last = (values.len() - 1) as f32;
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            Pos2::new(
                rect.left() + rect.width() * i as f32 / last,
                middle - v.clamp(-1.0, 1.0) * amplitude,
            )
        })
        .collect();
    painter
        .with_clip_rect(rect)
        .add(Shape::line(points, Stroke::new(1.5, color)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn a_scope_draws_across_a_rect_and_ignores_too_few_points() {
        egui::__run_test_ui(|ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 30.0));
            paint_scope(ui.painter(), rect, &[0.0, 1.0, -1.0, 2.5], Color32::WHITE);
            paint_scope(ui.painter(), rect, &[0.5], Color32::WHITE);
            paint_scope(ui.painter(), rect, &[], Color32::WHITE);
        });
        assert_eq!(scope_points(300.0), 150);
        assert_eq!(scope_points(0.0), 2);
    }

    #[test]
    fn icon_buttons_are_square() {
        egui::__run_test_ui(|ui| {
            let r = ui.add(IconButton::new(egui_phosphor::regular::PLAY));
            assert_eq!(r.rect.width(), r.rect.height());
            assert_eq!(r.rect.width(), ICON_BUTTON_SIZE);
            let r = ui.add(IconButton::new(egui_phosphor::regular::STOP).selected(true));
            assert_eq!(r.rect.width(), r.rect.height());
            // A smaller one keeps being square.
            let r = ui.add(IconButton::new(egui_phosphor::regular::STOP).size(18.0));
            assert_eq!(r.rect.width(), 18.0);
            assert_eq!(r.rect.height(), 18.0);
        });
    }
}
