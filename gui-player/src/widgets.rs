//! Small shared widgets.

use eframe::egui::{Button, Response, RichText, Ui, Vec2, Widget};

/// Side of every icon button, in points. Icon buttons are square and the
/// icon sits in the middle.
pub const ICON_BUTTON_SIZE: f32 = 28.0;
const ICON_SIZE: f32 = 16.0;

/// A square (1:1) button with a centered icon; `selected` draws it pressed
/// (toggles such as loop and mute).
pub struct IconButton<'a> {
    icon: &'a str,
    selected: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: &'a str) -> Self {
        Self { icon, selected: false }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.add(
            Button::new(RichText::new(self.icon).size(ICON_SIZE))
                .min_size(Vec2::splat(ICON_BUTTON_SIZE))
                .selected(self.selected),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    #[test]
    fn icon_buttons_are_square() {
        egui::__run_test_ui(|ui| {
            let r = ui.add(IconButton::new(egui_phosphor::regular::PLAY));
            assert_eq!(r.rect.width(), r.rect.height());
            assert_eq!(r.rect.width(), ICON_BUTTON_SIZE);
            let r = ui.add(IconButton::new(egui_phosphor::regular::STOP).selected(true));
            assert_eq!(r.rect.width(), r.rect.height());
        });
    }
}
