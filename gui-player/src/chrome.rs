//! Window chrome and non-content states: menu bar, status bar, and the
//! empty / loading / failed screens.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_phosphor::regular;

use crate::dialogs::Dialog;
use crate::i18n::{t, tf};
use crate::errors::ErrorLog;
use crate::loader::Loaded;
use crate::widgets::IconButton;
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
    /// The file that could not be opened (the reason goes to the error log).
    Failed(&'a str),
    Ready(&'a Loaded),
}

/// Sliders shown at the right of the status bar when a composition is open.
pub struct StatusSliders<'a> {
    pub volume: &'a mut f32,
    /// The last audible volume, to go back to when un-muting.
    pub last_volume: &'a mut f32,
    pub zoom: &'a mut f32,
}

/// Mutes (volume to 0, remembering the volume it had) or, if already muted,
/// goes back to the last audible volume. Muted means a volume of 0, however it
/// got there, so dragging the slider up un-mutes too.
pub fn toggle_mute(volume: &mut f32, last_volume: &mut f32) {
    if *volume > 0.0 {
        *last_volume = *volume;
        *volume = 0.0;
    } else {
        *volume = if *last_volume > 0.0 { *last_volume } else { 1.0 };
    }
}

/// Registers the Phosphor icon font next to egui's default fonts.
pub fn install_icon_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

/// The bottom ribbon, left to right: file, integrity and samples; then, in
/// all the space that is left, the icon that opens the list of errors and, at
/// its right, the latest error (cut with "…" when it does not fit) — both only
/// while there are errors; then, on the right, the master volume and the zoom
/// sliders.
///
/// Returns `true` when the errors icon was clicked.
pub fn status_bar(
    ui: &mut egui::Ui,
    status: &StatusLine,
    sliders: Option<StatusSliders>,
    errors: &ErrorLog,
) -> bool {
    let mut open_errors = false;
    ui.horizontal(|ui| {
        // 1, 2 and 3: from the left.
        status_text(ui, status);
        // A line between them and the errors block, always there (the errors
        // themselves only show while there are any).
        ui.separator();

        // The rest, right to left: the first widget added ends up rightmost.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 7 and 6.
            if let Some(StatusSliders { volume, last_volume, zoom }) = sliders {
                ui.add(egui::Slider::new(zoom, STEP_WIDTH_RANGE).show_value(false));
                ui.label(t("status.zoom"));
                ui.separator();
                ui.add(egui::Slider::new(volume, 0.0..=1.0).show_value(false))
                    .on_hover_text(tf("status.volume", &[("percent", &format!("{:.0}", *volume * 100.0))]));
                // Remember the last audible volume, then the mute button: the
                // crossed-out speaker while muted (volume 0).
                if *volume > 0.0 {
                    *last_volume = *volume;
                }
                let muted = *volume <= 0.0;
                let (icon, tip) = if muted {
                    (regular::SPEAKER_SLASH, t("status.unmute"))
                } else {
                    (regular::SPEAKER_HIGH, t("status.mute"))
                };
                let side = ui.spacing().interact_size.y;
                if ui
                    .add(IconButton::new(icon).size(side))
                    .on_hover_text(tip)
                    .clicked()
                {
                    toggle_mute(volume, last_volume);
                }
                ui.separator();
            }

            // The middle block, laid out right to left: first the latest error,
            // filling all the space up to the icon and cut with "…"...
            let Some(latest) = errors.latest() else { return };
            let side = ui.spacing().interact_size.y;
            let width = (ui.available_width() - side - ui.spacing().item_spacing.x).max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(width, side),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // One tooltip, with the full text: the label's own one (which
                    // egui shows when the text is cut) is turned off.
                    ui.add(
                        egui::Label::new(RichText::new(&latest.text).color(ERR_COLOR))
                            .truncate()
                            .show_tooltip_when_elided(false),
                    )
                    .on_hover_text(&latest.text);
                },
            );
            // ...and then, at its left, the icon that opens the list.
            let icon = ui
                .add(IconButton::new(regular::BUG).color(ERR_COLOR).size(side))
                .on_hover_text(tf("errors.tooltip", &[("count", &errors.len().to_string())]));
            open_errors = icon.clicked();
        });
    });
    open_errors
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
        StatusLine::Failed(file) => {
            ui.label(*file);
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
        let mut errors = ErrorLog::default();
        egui::__run_test_ui(|ui| assert!(!status_bar(ui, &StatusLine::Empty, None, &errors)));
        errors.push("a long error message that will not fit in the middle of a small footer ribbon");
        errors.push("audio: could not get the device's default config");
        egui::__run_test_ui(|ui| {
            status_bar(ui, &StatusLine::Failed("a.bm1"), None, &errors);
        });
        let (mut zoom, mut volume, mut last_volume) = (16.0, 1.0, 1.0);
        egui::__run_test_ui(|ui| {
            status_bar(
                ui,
                &StatusLine::Empty,
                Some(StatusSliders { volume: &mut volume, last_volume: &mut last_volume, zoom: &mut zoom }),
                &errors,
            );
        });
    }

    #[test]
    fn muting_goes_to_zero_and_unmuting_goes_back_to_the_previous_volume() {
        let (mut volume, mut last) = (0.6, 1.0);
        toggle_mute(&mut volume, &mut last);
        assert_eq!(volume, 0.0);
        assert_eq!(last, 0.6);
        toggle_mute(&mut volume, &mut last);
        assert_eq!(volume, 0.6);
        // Dragged down to 0 by hand: it counts as muted, and the icon goes
        // back to the last audible volume.
        last = 0.3;
        volume = 0.0;
        toggle_mute(&mut volume, &mut last);
        assert_eq!(volume, 0.3);
        // Nothing to go back to: full volume.
        (volume, last) = (0.0, 0.0);
        toggle_mute(&mut volume, &mut last);
        assert_eq!(volume, 1.0);
    }

    #[test]
    fn menu_bar_reports_no_action_when_nothing_is_clicked() {
        let mut actions = None;
        egui::__run_test_ui(|ui| actions = Some(menu_bar(ui, &["/a/song.bm1"], true)));
        assert_eq!(actions, Some(MenuActions::default()));
    }
}
