//! Loading a composition for the UI, off the UI thread.
//!
//! The UI wants more than `bm_session::open` gives: it must still show a
//! composition whose samples are missing (with warnings), so loading is
//! staged — structure first, then the sample check, then (only if every
//! sample is usable) decoding and rendering the audio.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use bm_project::{Project, SampleReport};
use bm_session::Session;
use bm_timeline::ResolvedArrangement;

/// Everything the UI shows about an opened composition.
pub struct Loaded {
    pub project: Project,
    /// One entry per sample, in file order.
    pub sample_reports: Vec<SampleReport>,
    /// The resolved arrangement (grid, clips). Always available once the
    /// structure is valid, even if samples are missing.
    pub timeline: ResolvedArrangement,
    pub seconds_per_step: f64,
    /// Decoded and rendered audio. `None` when some sample can't be used
    /// (see `session_error`).
    pub session: Option<Session>,
    pub session_error: Option<String>,
}

impl Loaded {
    pub fn ok_sample_count(&self) -> usize {
        self.sample_reports.iter().filter(|r| r.outcome.is_ok()).count()
    }

    pub fn all_samples_ok(&self) -> bool {
        self.ok_sample_count() == self.sample_reports.len()
    }

    /// File name of the `.bm1`, for titles and the status bar.
    pub fn file_name(&self) -> String {
        file_name(&self.project.path)
    }

    /// Total length of the arrangement in seconds (from the timeline, so it
    /// is known even without audio).
    pub fn duration_seconds(&self) -> f64 {
        self.timeline.total_steps as f64 * self.seconds_per_step
    }
}

pub enum LoadOutcome {
    Loaded(Box<Loaded>),
    /// The file could not be read or its structure is invalid.
    Failed { path: PathBuf, message: String },
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Loads `path` synchronously (structure, sample check, audio).
pub fn load_blocking(path: &Path) -> LoadOutcome {
    let project = match bm_project::load(path) {
        Ok(p) => p,
        Err(err) => {
            return LoadOutcome::Failed {
                path: path.to_path_buf(),
                message: err.to_string(),
            };
        }
    };

    let sample_reports = bm_project::check_samples(&project.composition);
    let timeline = bm_timeline::resolve(&project.composition);
    let seconds_per_step = bm_timeline::seconds_per_step(&project.composition.metadata);

    let (session, session_error) = if sample_reports.iter().all(|r| r.outcome.is_ok()) {
        match bm_session::open(path) {
            Ok(s) => (Some(s), None),
            Err(err) => (None, Some(err.to_string())),
        }
    } else {
        (None, Some("some samples are missing or invalid".to_string()))
    };

    LoadOutcome::Loaded(Box::new(Loaded {
        project,
        sample_reports,
        timeline,
        seconds_per_step,
        session,
        session_error,
    }))
}

/// Loads `path` on a background thread and repaints `ctx` when done, so
/// the UI never freezes while samples are decoded and rendered.
pub fn spawn_load(path: PathBuf, ctx: eframe::egui::Context) -> Receiver<LoadOutcome> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = load_blocking(&path);
        // The receiver is dropped if the user opened another file meanwhile.
        let _ = tx.send(outcome);
        ctx.request_repaint();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1")
    }

    #[test]
    fn loads_the_demo_song_completely() {
        let LoadOutcome::Loaded(l) = load_blocking(&demo()) else {
            panic!("demo should load");
        };
        assert_eq!(l.file_name(), "song1.bm1");
        assert_eq!(l.sample_reports.len(), 5);
        assert!(l.all_samples_ok());
        assert!(l.session.is_some() && l.session_error.is_none());
        assert_eq!(l.timeline.tracks.len(), 5);
        assert!((l.duration_seconds() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn a_missing_sample_still_loads_but_without_audio() {
        let dir = std::env::temp_dir().join(format!("gui-player-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = r#"{
            "metadata": { "version": "1.0", "title": "t", "bpm": 120 },
            "samples": [ { "id": "ghost", "file": "ghost.wav" } ],
            "patterns": [ { "id": "p", "sample": "ghost", "steps": ["C4", null, null, null] } ],
            "arrangement": { "tracks": [ { "id": "t", "sequence": ["p"] } ] }
        }"#;
        let path = dir.join("song.bm1");
        std::fs::write(&path, song).unwrap();

        let outcome = load_blocking(&path);
        std::fs::remove_dir_all(&dir).ok();

        let LoadOutcome::Loaded(l) = outcome else {
            panic!("structure is valid, so it should still load");
        };
        assert_eq!(l.ok_sample_count(), 0);
        assert!(!l.all_samples_ok());
        assert!(l.session.is_none() && l.session_error.is_some());
        assert_eq!(l.timeline.tracks.len(), 1);
    }

    #[test]
    fn an_invalid_or_missing_file_fails_with_a_message() {
        let LoadOutcome::Failed { message, .. } = load_blocking(Path::new("/no/such/file.bm1"))
        else {
            panic!("should fail");
        };
        assert!(!message.is_empty());
    }
}
