//! Developer aid, off unless environment variables are set:
//!
//! - `BM_GUI_SCREENSHOT=<file.ppm>`: after a few frames, save a screenshot
//!   of the window as a binary PPM and quit. Lets the UI be checked (or
//!   documented) without a person looking at the screen.
//! - `BM_GUI_SELECT=sample:<id>` / `pattern:<id>`: start with that element
//!   selected, so the properties panel has something to show.
//! - `BM_GUI_PLAY_AT=<seconds>`: start playing from that position as soon
//!   as the composition is open (if there is an audio device).

use std::io::Write;
use std::path::PathBuf;

use eframe::egui;

use crate::view::{ListTab, Selection};

/// Frames to let the UI settle before capturing.
const WARMUP_FRAMES: u32 = 20;
/// Give up (and quit) if no screenshot arrives after this many frames.
const MAX_FRAMES: u32 = 600;

pub struct ScreenshotJob {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

impl ScreenshotJob {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("BM_GUI_SCREENSHOT")?;
        Some(Self {
            path: PathBuf::from(path),
            frames: 0,
            requested: false,
        })
    }

    /// Call once per frame.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        ctx.request_repaint();

        if !self.requested && self.frames >= WARMUP_FRAMES {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }

        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(err) = write_ppm(&self.path, &image) {
                eprintln!("screenshot failed: {err}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.frames > MAX_FRAMES {
            eprintln!("screenshot never arrived");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn write_ppm(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [w, h] = image.size;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(out, "P6\n{w} {h}\n255\n")?;
    for pixel in &image.pixels {
        out.write_all(&[pixel.r(), pixel.g(), pixel.b()])?;
    }
    out.flush()
}

/// Selection requested through `BM_GUI_SELECT`, with the list tab it lives in.
pub fn initial_selection() -> Option<(Selection, ListTab)> {
    parse_selection(&std::env::var("BM_GUI_SELECT").ok()?)
}

/// Start position requested through `BM_GUI_PLAY_AT`.
pub fn initial_play_at() -> Option<f64> {
    std::env::var("BM_GUI_PLAY_AT").ok()?.parse().ok()
}

/// Dialog requested through `BM_GUI_DIALOG` (`libraries`, `settings`, `about`).
pub fn initial_dialog() -> Option<crate::dialogs::Dialog> {
    use crate::dialogs::Dialog;
    match std::env::var("BM_GUI_DIALOG").ok()?.as_str() {
        "libraries" => Some(Dialog::Libraries),
        "settings" => Some(Dialog::Settings),
        "about" => Some(Dialog::About),
        "quit" => Some(Dialog::ConfirmQuit),
        _ => None,
    }
}

fn parse_selection(text: &str) -> Option<(Selection, ListTab)> {
    let (kind, id) = text.split_once(':')?;
    match kind {
        "sample" => Some((Selection::Sample(id.to_string()), ListTab::Samples)),
        "pattern" => Some((Selection::Pattern(id.to_string()), ListTab::Patterns)),
        // Just open a tab, with nothing selected.
        "tab" => match id {
            "metadata" => Some((Selection::None, ListTab::Metadata)),
            "samples" => Some((Selection::None, ListTab::Samples)),
            "patterns" => Some((Selection::None, ListTab::Patterns)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_selection_requests() {
        assert_eq!(
            parse_selection("pattern:saxA"),
            Some((Selection::Pattern("saxA".into()), ListTab::Patterns))
        );
        assert_eq!(
            parse_selection("sample:kick"),
            Some((Selection::Sample("kick".into()), ListTab::Samples))
        );
        assert_eq!(parse_selection("tab:patterns"), Some((Selection::None, ListTab::Patterns)));
        assert_eq!(parse_selection("tab:nope"), None);
        assert_eq!(parse_selection("nonsense"), None);
        assert_eq!(parse_selection("track:a"), None);
    }
}
