//! The transport ribbon (play, pause, stop, loop, position) and the thin
//! wrapper over the playback engine that drives it.
//!
//! [`Transport`] owns the `bm-playback` engine when there is audio to play
//! and an output device to play it on; otherwise it stays unavailable (with
//! the reason) and the controls are disabled instead of the app failing.

use bm_playback::Engine;
use eframe::egui::{self, Button, RichText};

use crate::fmt;
use crate::loader::Loaded;
use crate::panels::ERR_COLOR;
use crate::view::ViewState;

pub struct Transport {
    engine: Option<Engine>,
    error: Option<String>,
}

impl Transport {
    /// Opens the output device for the composition's rendered tracks.
    pub fn new(l: &Loaded) -> Self {
        let Some(session) = &l.session else {
            return Self::unavailable(
                l.session_error
                    .clone()
                    .unwrap_or_else(|| "no audio to play".to_string()),
            );
        };
        match Engine::new(session.tracks.iter().map(|t| &t.audio)) {
            Ok(engine) => Self {
                engine: Some(engine),
                error: None,
            },
            Err(err) => Self::unavailable(err.to_string()),
        }
    }

    fn unavailable(reason: String) -> Self {
        Self {
            engine: None,
            error: Some(reason),
        }
    }

    pub fn available(&self) -> bool {
        self.engine.is_some()
    }

    /// Why playback is unavailable, if it is.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_playing(&self) -> bool {
        self.engine.as_ref().is_some_and(Engine::is_playing)
    }

    pub fn position_seconds(&self) -> f64 {
        self.engine.as_ref().map_or(0.0, Engine::position_seconds)
    }

    pub fn duration_seconds(&self) -> f64 {
        self.engine.as_ref().map_or(0.0, Engine::duration_seconds)
    }

    /// The playback position for drawing the cursor; `None` when there is
    /// no engine.
    pub fn playhead(&self) -> Option<f64> {
        self.engine.as_ref().map(Engine::position_seconds)
    }

    pub fn play(&self) {
        if let Some(e) = &self.engine {
            e.play();
        }
    }

    pub fn pause(&self) {
        if let Some(e) = &self.engine {
            e.pause();
        }
    }

    pub fn stop(&self) {
        if let Some(e) = &self.engine {
            e.stop();
        }
    }

    pub fn toggle_play(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, seconds: f64) {
        if let Some(e) = &self.engine {
            e.seek_seconds(seconds);
        }
    }

    /// Pushes the view's mute flags and loop setting to the engine.
    pub fn sync(&self, view: &ViewState) {
        if let Some(e) = &self.engine {
            for (i, muted) in view.muted.iter().enumerate() {
                e.set_muted(i, *muted);
            }
            e.set_looping(view.looping);
        }
    }
}

/// The transport ribbon: Play / Pause / Stop / Loop, `elapsed / total`, and
/// a position slider over the whole song.
pub fn show(ui: &mut egui::Ui, transport: &Transport, view: &mut ViewState) {
    ui.horizontal(|ui| {
        let playing = transport.is_playing();
        let available = transport.available();

        if ui.add_enabled(available && !playing, Button::new("Play")).clicked() {
            transport.play();
        }
        if ui.add_enabled(available && playing, Button::new("Pause")).clicked() {
            transport.pause();
        }
        if ui.add_enabled(available, Button::new("Stop")).clicked() {
            transport.stop();
        }
        ui.add_enabled_ui(available, |ui| {
            ui.toggle_value(&mut view.looping, "Loop");
        });

        ui.add_space(8.0);
        let position = transport.position_seconds();
        let duration = transport.duration_seconds();
        ui.label(RichText::new(time_label(position, duration)).monospace());

        if let Some(reason) = transport.error() {
            ui.label(RichText::new(format!("no audio: {reason}")).color(ERR_COLOR));
        } else {
            let mut value = position;
            ui.spacing_mut().slider_width = (ui.available_width() - 12.0).max(60.0);
            let response = ui.add(
                egui::Slider::new(&mut value, 0.0..=duration.max(0.001)).show_value(false),
            );
            if response.changed() {
                transport.seek(value);
            }
        }
    });
}

/// `00:03.2 / 00:08.6`.
pub fn time_label(position: f64, duration: f64) -> String {
    format!("{} / {}", fmt::duration_mmss(position), fmt::duration_mmss(duration))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{load_blocking, LoadOutcome};
    use std::path::Path;

    fn demo() -> Box<Loaded> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../player/demos/songs/song1.bm1");
        match load_blocking(&path) {
            LoadOutcome::Loaded(l) => l,
            LoadOutcome::Failed { message, .. } => panic!("demo failed to load: {message}"),
        }
    }

    #[test]
    fn time_label_shows_elapsed_over_total() {
        assert_eq!(time_label(3.2, 8.58), "00:03.2 / 00:08.6");
    }

    #[test]
    fn availability_and_error_are_always_consistent() {
        // Whether this machine has an audio device or not, the transport
        // must be either usable or carry the reason it is not.
        let l = demo();
        let t = Transport::new(&l);
        assert_eq!(t.available(), t.error().is_none());
        if t.available() {
            assert!(t.duration_seconds() > 2.0);
            assert_eq!(t.position_seconds(), 0.0);
            assert!(!t.is_playing());
        } else {
            assert_eq!(t.playhead(), None);
        }
    }

    #[test]
    fn a_composition_without_audio_gives_an_unavailable_transport() {
        let mut l = demo();
        l.session = None;
        l.session_error = Some("some samples are missing or invalid".into());
        let t = Transport::new(&l);
        assert!(!t.available());
        assert_eq!(t.error(), Some("some samples are missing or invalid"));
        // Controls are safe no-ops.
        t.play();
        t.toggle_play();
        t.seek(1.0);
        assert!(!t.is_playing());
    }

    #[test]
    fn the_ribbon_draws_available_or_not_without_panicking() {
        let l = demo();
        let mut view = ViewState::new(&l);
        let t = Transport::new(&l);
        egui::__run_test_ui(|ui| show(ui, &t, &mut view));

        let mut no_audio = demo();
        no_audio.session = None;
        let t = Transport::new(&no_audio);
        egui::__run_test_ui(|ui| show(ui, &t, &mut view));
    }
}
