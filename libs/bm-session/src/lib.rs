//! A thin facade over the pipeline: *load -> resolve -> decode samples ->
//! render*. It contains no logic of its own; it only chains the other
//! libraries so applications don't each re-implement the same sequence.
//!
//! It never prints. Everything an application may want to show (timeline,
//! decoded samples, per-track buffers, the mix) is exposed as data.

use std::collections::HashMap;
use std::path::Path;

use bm_dsp::AudioBuffer;
use bm_project::{Project, ProjectError, SampleSource};
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
    /// Decoded samples, by sample id, each at its own real channel count
    /// (not downmixed).
    pub samples: HashMap<String, AudioBuffer>,
    /// One rendered buffer per track, in arrangement order, every one at
    /// [`channels`](Self::channels) channels.
    pub tracks: Vec<TrackBuffer>,
    /// The mix of all tracks (nothing muted), normalized, interleaved at
    /// `channels` channels.
    pub master: Vec<f32>,
    /// How many channels `master` (and every track) has: the widest
    /// channel count among the composition's own samples (`1` if they are
    /// all mono).
    pub channels: u16,
}

impl Session {
    /// Total duration of the mixed audio, in seconds.
    pub fn master_duration_seconds(&self) -> f64 {
        let frames = self.master.len() / self.channels.max(1) as usize;
        frames as f64 / bm_render::OUTPUT_SAMPLE_RATE as f64
    }
}

/// Opens a `.bm1` from disk, resolves its arrangement, decodes every sample
/// and renders every track.
pub fn open(path: &Path) -> Result<Session, SessionError> {
    open_project(bm_project::load(path)?)
}

/// The same pipeline as [`open`], but starting from a [`Project`] already
/// loaded — from disk ([`bm_project::load`]) or from a `.bmz` package
/// ([`bm_project::load_bmz`]). Each sample is read from `project.source`,
/// keeping its real channel count (a mono composition renders and plays
/// exactly as before; one with a stereo sample mixes and plays in stereo,
/// upmixing any mono sample alongside it — see `bm_render::render_tracks_multi`).
pub fn open_project(project: Project) -> Result<Session, SessionError> {
    let composition = &project.composition;

    let timeline = bm_timeline::resolve(composition);
    let seconds_per_step = bm_timeline::seconds_per_step(&composition.metadata);

    let mut samples = HashMap::new();
    for sample in &composition.samples {
        let audio = match &project.source {
            SampleSource::Disk => bm_wav::load_wav_multi(Path::new(&sample.file)),
            SampleSource::Archive(entries) => match entries.get(&sample.file) {
                Some(bytes) => bm_wav::load_wav_bytes_multi(bytes, &sample.file),
                None => Err(bm_wav::missing(&sample.file)),
            },
        }
        .map_err(|source| SessionError::Sample { sample_id: sample.id.clone(), source })?;
        samples.insert(sample.id.clone(), audio);
    }

    let channels = samples.values().map(AudioBuffer::channels).max().unwrap_or(1);
    let tracks =
        bm_render::render_tracks_multi(&timeline, &composition.samples, &samples, seconds_per_step, channels);
    let master = bm_render::mix_tracks(&tracks, &[]);

    Ok(Session {
        project,
        timeline,
        seconds_per_step,
        samples,
        tracks,
        master,
        channels,
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
        // The demo's sax sample is genuinely stereo, so the whole session
        // mixes at 2 channels: every track and the master follow it.
        assert_eq!(session.channels, 2);
        assert!(session.samples["sax"].channels() >= 2);
        for track in &session.tracks {
            assert_eq!(track.audio.channels(), 2);
        }
        // The mix is exactly as long as the longest rendered track, and
        // covers most of the 2.5 s arrangement (its last notes end just
        // before the arrangement does).
        let longest = session.tracks.iter().map(|t| t.audio.data.len()).max().unwrap();
        assert_eq!(session.master.len(), longest);
        assert!(session.master_duration_seconds() > 2.0);
    }

    #[test]
    fn a_composition_with_only_mono_samples_mixes_at_one_channel() {
        let dir = std::env::temp_dir().join(format!("bm-session-mono-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = dir.join("blip.wav");
        bm_wav::write_wav(&wav_path, &[0.1, 0.2, -0.1, -0.2], 44_100).unwrap();

        let song = r#"{
            "metadata": { "version": "1.0", "title": "mono", "bpm": 120 },
            "samples": [ { "id": "blip", "file": "blip.wav" } ],
            "patterns": [ { "id": "p", "sample": "blip", "steps": ["C4", null, null, null] } ],
            "arrangement": { "tracks": [ { "id": "t", "sequence": ["p"] } ] }
        }"#;
        std::fs::write(dir.join("song.bm1"), song).unwrap();

        let session = open(&dir.join("song.bm1")).expect("mono song should open");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(session.channels, 1);
        assert_eq!(session.samples["blip"].channels(), 1);
        for track in &session.tracks {
            assert_eq!(track.audio.channels(), 1);
        }
    }

    #[test]
    fn opens_a_packaged_bmz_the_same_way_as_the_bm1_it_came_from() {
        let project = bm_project::load(&demo()).unwrap();
        let out = std::env::temp_dir().join(format!("bm-session-pkg-{}.bmz", std::process::id()));
        bm_project::write_package(&project, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        std::fs::remove_file(&out).ok();

        let packaged = bm_project::load_bmz(&bytes, out).unwrap();
        let session = open_project(packaged).expect("the package should open like the original");
        assert_eq!(session.samples.len(), 5);
        assert_eq!(session.project.composition.metadata.title, "Demo");
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
