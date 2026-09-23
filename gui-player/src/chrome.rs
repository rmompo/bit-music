//! Window chrome and non-content states: menu bar, status bar, and the
//! empty / loading / failed screens.

use eframe::egui::{self, RichText};

use crate::loader::Loaded;
use crate::panels::{ERR_COLOR, OK_COLOR};
use crate::view::STEP_WIDTH_RANGE;

/// What the user asked for through the menu.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MenuActions {
    pub open: bool,
    pub quit: bool,
}

pub fn menu_bar(ui: &mut egui::Ui) -> MenuActions {
    let mut actions = MenuActions::default();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Open…").clicked() {
                actions.open = true;
                ui.close();
            }
            if ui.button("Quit").clicked() {
                actions.quit = true;
                ui.close();
            }
        });
    });
    actions
}

/// What the status bar reports.
pub enum StatusLine<'a> {
    Empty,
    Loading(&'a str),
    Failed { file: &'a str, message: &'a str },
    Ready(&'a Loaded),
}

/// The bottom ribbon: status on the left and, when a composition is open,
/// the arrangement's zoom slider on the right.
pub fn status_bar(ui: &mut egui::Ui, status: &StatusLine, zoom: Option<&mut f32>) {
    ui.horizontal(|ui| {
        // Status text first, from the left...
        status_text(ui, status);
        // ...then the zoom, right-aligned in whatever space is left.
        if let Some(zoom) = zoom {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Slider::new(zoom, STEP_WIDTH_RANGE).show_value(false));
                ui.label("Zoom");
            });
        }
    });
}

fn status_text(ui: &mut egui::Ui, status: &StatusLine) {
    match status {
        StatusLine::Empty => {
            ui.label("No file open");
        }
        StatusLine::Loading(file) => {
            ui.spinner();
            ui.label(format!("Loading {file}…"));
        }
        StatusLine::Failed { file, message } => {
            ui.label(RichText::new(format!("Could not open {file}: {message}")).color(ERR_COLOR));
        }
        StatusLine::Ready(l) => {
            ui.label(l.file_name());
            ui.separator();
            ui.label(RichText::new("integrity OK").color(OK_COLOR));
            ui.separator();
            let text = format!("{}/{} samples", l.ok_sample_count(), l.sample_reports.len());
            let color = if l.all_samples_ok() { OK_COLOR } else { ERR_COLOR };
            ui.label(RichText::new(text).color(color));
        }
    }
}

pub fn empty_state(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(
            RichText::new("Open a .bm1 composition (File > Open…) or drop one here")
                .heading()
                .weak(),
        );
    });
}

pub fn loading_state(ui: &mut egui::Ui, file: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.spinner();
        ui.label(format!("Loading {file}…"));
    });
}

pub fn failed_state(ui: &mut egui::Ui, file: &str, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(RichText::new(format!("Could not open {file}")).heading().color(ERR_COLOR));
        ui.label(message);
        ui.label(RichText::new("Open another file with File > Open… or drop one here").weak());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_content_screens_draw_without_panicking() {
        egui::__run_test_ui(|ui| empty_state(ui));
        egui::__run_test_ui(|ui| loading_state(ui, "song.bm1"));
        egui::__run_test_ui(|ui| failed_state(ui, "song.bm1", "boom"));
        egui::__run_test_ui(|ui| status_bar(ui, &StatusLine::Empty, None));
        let mut zoom = 16.0;
        egui::__run_test_ui(|ui| status_bar(ui, &StatusLine::Empty, Some(&mut zoom)));
        egui::__run_test_ui(|ui| {
            status_bar(ui, &StatusLine::Failed { file: "a.bm1", message: "bad" }, None)
        });
    }

    #[test]
    fn menu_bar_reports_no_action_when_nothing_is_clicked() {
        let mut actions = None;
        egui::__run_test_ui(|ui| actions = Some(menu_bar(ui)));
        assert_eq!(actions, Some(MenuActions::default()));
    }
}
