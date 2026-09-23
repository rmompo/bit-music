//! A thin facade over the pipeline: *load -> resolve -> decode samples ->
//! render*. It contains no logic of its own; it only chains the other
//! libraries so applications don't each re-implement the same sequence.
//!
//! It never prints. Everything an application may want to show (timeline,
//! decoded samples, per-track buffers, the mix) is exposed as data.

use std::collections::HashMap;
use std::path::Path;

use bm_dsp::AudioBuffer;
use bm_project::{Project, ProjectError};
use bm_render::TrackBuffer;
use bm_timeline::ResolvedArrangement;
use bm_wav::WavError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    Project(#[from] ProjectError),

    #[error("could not load sample '{sample_id}': {source}")]
    Sample {
        sample_id: String,
        #[source]
        source: WavError,
    },
}

/// A project that has been fully loaded, resolved and rendered.
#[derive(Debug)]
pub struct Session {
    pub project: Project,
    /// The resolved arrangement (grid, clips, per-step notes).
    pub timeline: ResolvedArrangement,
    pub seconds_per_step: f64,
    /// Decoded samples, by sample id.
    pub samples: HashMap<String, AudioBuffer>,
    /// One rendered buffer per track, in arrangement order.
    pub tracks: Vec<TrackBuffer>,
    /// The mix of all tracks (nothing muted), normalized.
    pub master: Vec<f32>,
}

impl Session {
    /// Total duration of the mixed audio, in seconds.
    pub fn master_duration_seconds(&self) -> f64 {
        self.master.len() as f64 / bm_render::OUTPUT_SAMPLE_RATE as f64
    }
}

/// Opens a `.bm1`, resolves its arrangement, decodes every sample and
/// renders every track.
pub fn open(path: &Path) -> Result<Session, SessionError> {
    let project = bm_project::load(path)?;
    let composition = &project.composition;

    let timeline = bm_timeline::resolve(composition);
    let seconds_per_step = bm_timeline::seconds_per_step(&composition.metadata);

    let mut samples = HashMap::new();
    for sample in &composition.samples {
        let audio = bm_wav::load_wav(Path::new(&sample.file)).map_err(|source| {
            SessionError::Sample {
                sample_id: sample.id.clone(),
                source,
            }
        })?;
        samples.insert(sample.id.clone(), audio);
    }

    let tracks = bm_render::render_tracks(&timeline, &composition.samples, &samples, seconds_per_step);
    let master = bm_render::mix_tracks(&tracks, &[]);

    Ok(Session {
        project,
        timeline,
        seconds_per_step,
        samples,
        tracks,
        master,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/songs/song1.bm1")
    }

    #[test]
    fn opens_the_demo_song_end_to_end() {
        let session = open(&demo()).expect("demo song should open");
        assert_eq!(session.project.composition.metadata.title, "Demo");
        assert_eq!(session.samples.len(), 5);
        assert_eq!(session.tracks.len(), 5);
        assert_eq!(session.timeline.total_steps, 20);
        // The mix is exactly as long as the longest rendered track, and
        // covers most of the 2.5 s arrangement (its last notes end just
        // before the arrangement does).
        let longest = session.tracks.iter().map(|t| t.audio.data.len()).max().unwrap();
        assert_eq!(session.master.len(), longest);
        assert!(session.master_duration_seconds() > 2.0);
    }

    #[test]
    fn a_missing_sample_is_reported_with_its_id() {
        let dir = std::env::temp_dir().join(format!("bm-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = r#"{
            "metadata": { "version": "1.0", "title": "t", "bpm": 120 },
            "samples": [ { "id": "ghost", "file": "ghost.wav" } ],
            "patterns": [ { "id": "p", "sample": "ghost", "steps": ["C4", null, null, null] } ],
            "arrangement": { "tracks": [ { "id": "t", "sequence": ["p"] } ] }
        }"#;
        std::fs::write(dir.join("song.bm1"), song).unwrap();

        let err = open(&dir.join("song.bm1")).unwrap_err();
        std::fs::remove_dir_all(&dir).ok();
        assert!(matches!(err, SessionError::Sample { ref sample_id, .. } if sample_id == "ghost"));
    }
}
