//! Window chrome and non-content states: menu bar, status bar, and the
//! empty / loading / failed screens.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_phosphor::regular;

use crate::dialogs::Dialog;
use crate::i18n::{t, tf};
use crate::loader::Loaded;
use crate::panels::{ERR_COLOR, OK_COLOR};
use crate::view::STEP_WIDTH_RANGE;

/// What the user asked for through the menu.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MenuActions {
    pub open: bool,
    pub quit: bool,
    /// Tools > Export > WAV.
    pub export_wav: bool,
    /// A file picked from File > Open recent.
    pub open_path: Option<PathBuf>,
    /// A modal dialog the user asked for (Tools / Help menus).
    pub dialog: Option<Dialog>,
}

/// `recent` are the paths for File > Open recent, newest first; `can_export`
/// says whether there is a composition with audio to export.
pub fn menu_bar(ui: &mut egui::Ui, recent: &[&str], can_export: bool) -> MenuActions {
    let mut actions = MenuActions::default();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(t("menu.file"), |ui| {
            if ui.button(t("menu.open")).clicked() {
                actions.open = true;
                ui.close();
            }
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button(t("menu.open_recent"), |ui| {
                    for path in recent {
                        if ui.button(*path).clicked() {
                            actions.open_path = Some(PathBuf::from(path));
                            ui.close();
                        }
                    }
                });
            });
            if ui.button(t("menu.quit")).clicked() {
                actions.quit = true;
                ui.close();
            }
        });
        ui.menu_button(t("menu.tools"), |ui| {
            ui.menu_button(t("menu.export"), |ui| {
                if ui
                    .add_enabled(can_export, egui::Button::new(t("menu.export_wav")))
                    .clicked()
                {
                    actions.export_wav = true;
                    ui.close();
                }
            });
            if ui.button(t("menu.settings")).clicked() {
                actions.dialog = Some(Dialog::Settings);
                ui.close();
            }
        });
        ui.menu_button(t("menu.help"), |ui| {
            if ui.button(t("menu.libraries")).clicked() {
                actions.dialog = Some(Dialog::Libraries);
                ui.close();
            }
            if ui.button(t("menu.about")).clicked() {
                actions.dialog = Some(Dialog::About);
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

/// Sliders shown at the right of the status bar when a composition is open.
pub struct StatusSliders<'a> {
    pub volume: &'a mut f32,
    pub zoom: &'a mut f32,
}

/// Registers the Phosphor icon font next to egui's default fonts.
pub fn install_icon_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

/// The bottom ribbon: status on the left and, when a composition is open,
/// the master volume and the arrangement's zoom sliders on the right.
pub fn status_bar(ui: &mut egui::Ui, status: &StatusLine, sliders: Option<StatusSliders>) {
    ui.horizontal(|ui| {
        // Status text first, from the left...
        status_text(ui, status);
        // ...then the zoom, right-aligned in whatever space is left.
        if let Some(StatusSliders { volume, zoom }) = sliders {
            // Right-to-left: the first widget added ends up rightmost.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Slider::new(zoom, STEP_WIDTH_RANGE).show_value(false));
                ui.label(t("status.zoom"));
                ui.separator();
                ui.add(egui::Slider::new(volume, 0.0..=1.0).show_value(false))
                    .on_hover_text(tf("status.volume", &[("percent", &format!("{:.0}", *volume * 100.0))]));
                ui.label(regular::SPEAKER_HIGH);
            });
        }
    });
}

fn status_text(ui: &mut egui::Ui, status: &StatusLine) {
    match status {
        StatusLine::Empty => {
            ui.label(t("status.no_file"));
        }
        StatusLine::Loading(file) => {
            ui.spinner();
            ui.label(tf("status.loading", &[("file", file)]));
        }
        StatusLine::Failed { file, message } => {
            ui.label(RichText::new(tf("status.could_not_open", &[("file", file), ("message", message)])).color(ERR_COLOR));
        }
        StatusLine::Ready(l) => {
            ui.label(l.file_name());
            ui.separator();
            ui.label(RichText::new(t("status.integrity_ok")).color(OK_COLOR));
            ui.separator();
            let text = tf(
                "status.samples",
                &[("ok", &l.ok_sample_count().to_string()), ("total", &l.sample_reports.len().to_string())],
            );
            let color = if l.all_samples_ok() { OK_COLOR } else { ERR_COLOR };
            ui.label(RichText::new(text).color(color));
        }
    }
}

pub fn empty_state(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(
            RichText::new(t("empty.hint"))
                .heading()
                .weak(),
        );
    });
}

pub fn loading_state(ui: &mut egui::Ui, file: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.spinner();
        ui.label(tf("status.loading", &[("file", file)]));
    });
}

pub fn failed_state(ui: &mut egui::Ui, file: &str, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(RichText::new(tf("failed.title", &[("file", file)])).heading().color(ERR_COLOR));
        ui.label(message);
        ui.label(RichText::new(t("failed.hint")).weak());
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
        let (mut zoom, mut volume) = (16.0, 1.0);
        egui::__run_test_ui(|ui| {
            status_bar(
                ui,
                &StatusLine::Empty,
                Some(StatusSliders { volume: &mut volume, zoom: &mut zoom }),
            )
        });
        egui::__run_test_ui(|ui| {
            status_bar(ui, &StatusLine::Failed { file: "a.bm1", message: "bad" }, None)
        });
    }

    #[test]
    fn menu_bar_reports_no_action_when_nothing_is_clicked() {
        let mut actions = None;
        egui::__run_test_ui(|ui| actions = Some(menu_bar(ui, &["/a/song.bm1"], true)));
        assert_eq!(actions, Some(MenuActions::default()));
    }
}
