//! Offline rendering: turns a resolved timeline plus decoded samples into
//! audio, before anything plays.
//!
//! This is the "pre-render" strategy documented in `specs/architecture.md`:
//! all the heavy computation (pitch-shift resampling, voice summing) happens
//! here, so no computation pause can affect the sync between channels during
//! playback. Rendering is split per track so a player can mute or solo a
//! track at playback time without re-rendering.

pub mod pitch;

use std::collections::HashMap;

use bm_dsp::{self as dsp, AudioBuffer};
use bm_format::model::{Pattern, Sample};
use bm_format::note::{parse_note, Note};
use bm_timeline::ResolvedArrangement;

pub use pitch::{pitch_shift_for, PitchShift};

/// Sample rate of every rendered buffer. If the real output device uses a
/// different one, the playback layer does the final conversion.
pub const OUTPUT_SAMPLE_RATE: u32 = 44100;

/// The rendered audio of a single track.
#[derive(Debug, Clone)]
pub struct TrackBuffer {
    pub track_id: String,
    /// Mono audio at [`OUTPUT_SAMPLE_RATE`]. Not normalized: a track can
    /// exceed `[-1, 1]` when its own voices overlap; normalization happens
    /// on the mix.
    pub audio: AudioBuffer,
}

/// Renders every track of `arrangement` to its own buffer, applying each
/// step's pitch-shift and summing the voices that overlap in time within
/// the track.
///
/// `audio` maps a sample id to its decoded audio; every sample referenced
/// by the arrangement must be present (guaranteed if the composition was
/// validated and all its samples were loaded).
pub fn render_tracks(
    arrangement: &ResolvedArrangement,
    samples: &[Sample],
    audio: &HashMap<String, AudioBuffer>,
    seconds_per_step: f64,
) -> Vec<TrackBuffer> {
    let samples_by_id: HashMap<&str, &Sample> =
        samples.iter().map(|s| (s.id.as_str(), s)).collect();
    let frames_per_step = seconds_per_step * OUTPUT_SAMPLE_RATE as f64;

    arrangement
        .tracks
        .iter()
        .map(|track| {
            let mut buffer: Vec<f32> = Vec::new();

            for (index, step) in track.steps.iter().enumerate() {
                let Some(step) = step else { continue };

                let sample = samples_by_id[step.sample_id.as_str()];
                let source = &audio[&step.sample_id];
                let voice = render_voice(sample, source, &step.note);

                let start = (index as f64 * frames_per_step).round() as usize;
                dsp::mix_into(&mut buffer, &voice, start);
            }

            TrackBuffer {
                track_id: track.id.clone(),
                audio: AudioBuffer::new(buffer, OUTPUT_SAMPLE_RATE),
            }
        })
        .collect()
}

/// One sounding note: the sample pitch-shifted to `note` and converted to
/// [`OUTPUT_SAMPLE_RATE`] in a single resampling pass.
fn render_voice(sample: &Sample, source: &AudioBuffer, note: &Note) -> Vec<f32> {
    let pitch_shift = pitch::pitch_shift_for(sample, note);
    // combined ratio: pitch-shift + conversion from the .wav's native
    // sample rate to the output sample rate.
    let rate_ratio = source.sample_rate as f64 / OUTPUT_SAMPLE_RATE as f64;
    dsp::resample(&source.data, pitch_shift.ratio * rate_ratio)
}

/// Renders a single pattern on its own, once, from its first step (for
/// auditioning it). `None` if its sample is unknown or not in `audio`.
/// Steps that are not valid notes are skipped (validation rejects them
/// before a composition gets this far).
pub fn render_pattern(
    pattern: &Pattern,
    samples: &[Sample],
    audio: &HashMap<String, AudioBuffer>,
    seconds_per_step: f64,
) -> Option<AudioBuffer> {
    let sample = samples.iter().find(|s| s.id == pattern.sample)?;
    let source = audio.get(&pattern.sample)?;
    let frames_per_step = seconds_per_step * OUTPUT_SAMPLE_RATE as f64;

    let mut buffer: Vec<f32> = Vec::new();
    for (index, raw) in pattern.steps.iter().enumerate() {
        let Some(raw) = raw else { continue };
        let Ok(note) = parse_note(raw) else { continue };
        let voice = render_voice(sample, source, &note);
        let start = (index as f64 * frames_per_step).round() as usize;
        dsp::mix_into(&mut buffer, &voice, start);
    }
    dsp::normalize(&mut buffer);
    Some(AudioBuffer::new(buffer, OUTPUT_SAMPLE_RATE))
}

/// Sums the tracks into a single mono buffer, skipping the ones flagged in
/// `muted` (missing entries count as not muted), and normalizes the result
/// if its peak exceeds `1.0`.
pub fn mix_tracks(tracks: &[TrackBuffer], muted: &[bool]) -> Vec<f32> {
    let mut master: Vec<f32> = Vec::new();
    for (i, track) in tracks.iter().enumerate() {
        if muted.get(i).copied().unwrap_or(false) {
            continue;
        }
        dsp::mix_into(&mut master, &track.audio.data, 0);
    }
    dsp::normalize(&mut master);
    master
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_format::parse_composition;
    use bm_timeline::{resolve, seconds_per_step};

    const SONG: &str = r#"{
        "metadata": { "version": "1.0", "title": "t", "bpm": 120 },
        "samples": [ { "id": "s", "file": "s.wav" } ],
        "patterns": [
            { "id": "p", "sample": "s", "steps": ["C4", null, null, null, "C4", null, null, null] }
        ],
        "arrangement": { "tracks": [
            { "id": "a", "sequence": ["p"] },
            { "id": "b", "sequence": ["p"] }
        ] }
    }"#;

    fn render() -> Vec<TrackBuffer> {
        let composition = parse_composition(SONG).unwrap();
        let arrangement = resolve(&composition);
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 10], OUTPUT_SAMPLE_RATE));
        render_tracks(
            &arrangement,
            &composition.samples,
            &audio,
            seconds_per_step(&composition.metadata),
        )
    }

    #[test]
    fn each_track_is_rendered_to_its_own_buffer_at_the_right_offsets() {
        let tracks = render();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].track_id, "a");
        // 120 bpm, 4 steps/beat -> 0.125 s/step -> step 4 starts at frame 22050.
        let data = &tracks[0].audio.data;
        assert_eq!(data[0], 0.5);
        assert_eq!(data[1000], 0.0);
        assert_eq!(data[22050], 0.5);
        assert_eq!(data.len(), 22050 + 10);
    }

    #[test]
    fn mixing_sums_tracks_and_muting_removes_them() {
        let tracks = render();

        // two identical 0.5 voices sum to 1.0 (no normalization needed)
        let both = mix_tracks(&tracks, &[]);
        assert_eq!(both[0], 1.0);

        let only_b = mix_tracks(&tracks, &[true, false]);
        assert_eq!(only_b[0], 0.5);

        let none = mix_tracks(&tracks, &[true, true]);
        assert!(none.is_empty());
    }

    #[test]
    fn a_pattern_renders_on_its_own_and_unknown_samples_give_none() {
        let c = parse_composition(SONG).unwrap();
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 100], 44100));
        let sps = 0.01;
        let buf = render_pattern(&c.patterns[0], &c.samples, &audio, sps).unwrap();
        assert_eq!(buf.sample_rate, OUTPUT_SAMPLE_RATE);
        // two notes, the second starting 4 steps in
        let start = (4.0 * sps * OUTPUT_SAMPLE_RATE as f64).round() as usize;
        assert!(buf.data.len() >= start + 100);
        assert!(buf.data[start] != 0.0);
        assert!(render_pattern(&c.patterns[0], &c.samples, &HashMap::new(), sps).is_none());
    }
}
