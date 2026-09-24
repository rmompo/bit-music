//! The transport ribbon (play, pause, stop, loop, position) and the thin
//! wrapper over the playback engine that drives it.
//!
//! [`Transport`] owns the `bm-playback` engine when there is audio to play
//! and an output device to play it on; otherwise it stays unavailable (with
//! the reason) and the controls are disabled instead of the app failing.

use bm_playback::Engine;
use bm_render::render_pattern;
use bm_dsp::AudioBuffer;
use eframe::egui::{self, RichText};
use egui_phosphor::regular;

use crate::fmt;
use crate::widgets::IconButton;
use crate::loader::Loaded;
use crate::panels::ERR_COLOR;
use crate::view::ViewState;

fn sample_key(id: &str) -> String {
    format!("sample:{id}")
}

fn pattern_key(id: &str) -> String {
    format!("pattern:{id}")
}

pub struct Transport {
    engine: Option<Engine>,
    /// Keys (`sample:<id>` / `pattern:<id>`), in the order of the engine's previews.
    preview_ids: Vec<String>,
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
        // Previews: every sample as it is, and every pattern rendered on
        // its own (with the composition's tempo).
        let c = &l.project.composition;
        let mut preview_ids: Vec<String> = Vec::new();
        let mut previews: Vec<AudioBuffer> = Vec::new();
        for s in &c.samples {
            if let Some(audio) = session.samples.get(&s.id) {
                preview_ids.push(sample_key(&s.id));
                previews.push(audio.clone());
            }
        }
        for p in &c.patterns {
            if let Some(audio) =
                render_pattern(p, &c.samples, &session.samples, l.seconds_per_step)
            {
                preview_ids.push(pattern_key(&p.id));
                previews.push(audio);
            }
        }
        match Engine::with_previews(session.tracks.iter().map(|t| &t.audio), &previews) {
            Ok(engine) => Self {
                engine: Some(engine),
                preview_ids,
                error: None,
            },
            Err(err) => Self::unavailable(err.to_string()),
        }
    }

    fn unavailable(reason: String) -> Self {
        Self {
            engine: None,
            preview_ids: Vec::new(),
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

    /// Whether sample `id` can be played on its own.
    pub fn can_preview_sample(&self, id: &str) -> bool {
        self.can_preview(&sample_key(id))
    }

    /// Whether pattern `id` can be played on its own.
    pub fn can_preview_pattern(&self, id: &str) -> bool {
        self.can_preview(&pattern_key(id))
    }

    /// Plays sample `id` once, at its original pitch, on top of the
    /// transport (which is not touched).
    pub fn preview_sample(&self, id: &str) {
        self.preview(&sample_key(id));
    }

    /// Plays pattern `id` once from its start, on top of the transport.
    pub fn preview_pattern(&self, id: &str) {
        self.preview(&pattern_key(id));
    }

    /// Whether sample `id` is sounding as a preview right now.
    pub fn is_previewing_sample(&self, id: &str) -> bool {
        self.is_previewing(&sample_key(id))
    }

    /// Whether pattern `id` is sounding as a preview right now.
    pub fn is_previewing_pattern(&self, id: &str) -> bool {
        self.is_previewing(&pattern_key(id))
    }

    /// Whether any preview is sounding (the UI keeps repainting so its
    /// play buttons come back when it ends).
    pub fn any_preview_playing(&self) -> bool {
        self.engine.as_ref().is_some_and(Engine::any_preview_playing)
    }

    fn is_previewing(&self, key: &str) -> bool {
        match (&self.engine, self.preview_ids.iter().position(|p| p == key)) {
            (Some(e), Some(i)) => e.is_preview_playing(i),
            _ => false,
        }
    }

    /// Oscilloscope values (`points` of them) for sample `id` while its
    /// preview sounds; empty otherwise.
    pub fn preview_scope_sample(&self, id: &str, points: usize) -> Vec<f32> {
        self.preview_scope(&sample_key(id), points)
    }

    /// The same for pattern `id`.
    pub fn preview_scope_pattern(&self, id: &str, points: usize) -> Vec<f32> {
        self.preview_scope(&pattern_key(id), points)
    }

    /// One oscilloscope trace per track while the transport plays (empty for
    /// a track that is muted, or when nothing plays).
    pub fn track_scopes(&self, points: usize) -> Vec<Vec<f32>> {
        match &self.engine {
            Some(e) => (0..e.track_count()).map(|i| e.track_scope(i, points)).collect(),
            None => Vec::new(),
        }
    }

    fn preview_scope(&self, key: &str, points: usize) -> Vec<f32> {
        match (&self.engine, self.preview_ids.iter().position(|p| p == key)) {
            (Some(e), Some(i)) => e.preview_scope(i, points),
            _ => Vec::new(),
        }
    }

    fn can_preview(&self, key: &str) -> bool {
        self.engine.is_some() && self.preview_ids.iter().any(|p| p == key)
    }

    fn preview(&self, key: &str) {
        if let (Some(e), Some(i)) = (&self.engine, self.preview_ids.iter().position(|p| p == key)) {
            e.play_preview(i);
        }
    }

    /// Pushes the view's mute flags, loop setting and volume to the engine.
    pub fn sync(&self, view: &ViewState) {
        if let Some(e) = &self.engine {
            for (i, muted) in view.muted.iter().enumerate() {
                e.set_muted(i, *muted);
            }
            e.set_looping(view.looping);
            e.set_volume(view.volume);
        }
    }
}

/// The transport ribbon: play/pause toggle, stop, loop, `elapsed / total`, and
/// a position slider over the whole song.
pub fn show(ui: &mut egui::Ui, transport: &Transport, view: &mut ViewState) {
    ui.horizontal(|ui| {
        let playing = transport.is_playing();
        let available = transport.available();

        // One toggle: shows what a click will do. It goes back to "play"
        // by itself when playback stops or reaches the end.
        let (icon, tip) = if playing {
            (regular::PAUSE, "Pause (Space)")
        } else {
            (regular::PLAY, "Play (Space)")
        };
        let toggle = ui
            .add_enabled(available, IconButton::new(icon))
            .on_hover_text(tip);
        if toggle.clicked() {
            transport.toggle_play();
        }
        let stop = ui
            .add_enabled(available, IconButton::new(regular::STOP))
            .on_hover_text("Stop and rewind");
        if stop.clicked() {
            transport.stop();
        }
        ui.add_enabled_ui(available, |ui| {
            let looping = ui
                .add(IconButton::new(regular::REPEAT).selected(view.looping))
                .on_hover_text("Loop");
            if looping.clicked() {
                view.looping = !view.looping;
            }
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
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1");
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
