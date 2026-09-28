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
    /// The widest channel count among the samples this track actually
    /// plays (`1` for a silent or all-mono track) — *before* `audio` was
    /// upmixed to the whole composition's shared channel count, so a
    /// caller can still tell a genuinely mono track from a stereo one once
    /// every track has been widened to sit in the same mix (e.g. to size
    /// an oscilloscope's traces correctly).
    pub native_channels: u16,
}

/// Renders every track of `arrangement` to its own mono buffer. The same
/// as [`render_tracks_multi`] with `channels: 1` (every voice mixed in is
/// already mono, so there is nothing to upmix).
pub fn render_tracks(
    arrangement: &ResolvedArrangement,
    samples: &[Sample],
    audio: &HashMap<String, AudioBuffer>,
    seconds_per_step: f64,
) -> Vec<TrackBuffer> {
    render_tracks_multi(arrangement, samples, audio, seconds_per_step, 1)
}

/// Renders every track of `arrangement` to its own buffer of `channels`
/// channels, applying each step's pitch-shift and summing the voices that
/// overlap in time within the track. A voice whose own sample has fewer
/// channels than `channels` is upmixed (see [`dsp::upmix`]) before being
/// mixed in, so e.g. a mono kick and a stereo sax can share a composition:
/// every track (and the master mix built from them) ends up at the same,
/// wider channel count.
///
/// `audio` maps a sample id to its decoded audio; every sample referenced
/// by the arrangement must be present (guaranteed if the composition was
/// validated and all its samples were loaded).
pub fn render_tracks_multi(
    arrangement: &ResolvedArrangement,
    samples: &[Sample],
    audio: &HashMap<String, AudioBuffer>,
    seconds_per_step: f64,
    channels: u16,
) -> Vec<TrackBuffer> {
    let channels = channels.max(1);
    let samples_by_id: HashMap<&str, &Sample> =
        samples.iter().map(|s| (s.id.as_str(), s)).collect();
    let frames_per_step = seconds_per_step * OUTPUT_SAMPLE_RATE as f64;

    arrangement
        .tracks
        .iter()
        .map(|track| {
            let mut buffer: Vec<f32> = Vec::new();
            let mut native_channels: u16 = 1;

            for (index, step) in track.steps.iter().enumerate() {
                let Some(step) = step else { continue };

                let sample = samples_by_id[step.sample_id.as_str()];
                let source = &audio[&step.sample_id];
                native_channels = native_channels.max(source.channels());
                let voice = render_voice(sample, source, &step.note, channels);

                let start = (index as f64 * frames_per_step).round() as usize;
                dsp::mix_into_multi(&mut buffer, &voice, start, channels as usize);
            }

            TrackBuffer {
                track_id: track.id.clone(),
                audio: AudioBuffer::new_multi(buffer, OUTPUT_SAMPLE_RATE, channels),
                native_channels,
            }
        })
        .collect()
}

/// One sounding note: the sample pitch-shifted to `note` and converted to
/// [`OUTPUT_SAMPLE_RATE`] in a single resampling pass (resampled per
/// channel, at the source's own channel count, never blending channels
/// together — see [`dsp::resample_multi`]), then upmixed to `channels`
/// channels if the source has fewer.
fn render_voice(sample: &Sample, source: &AudioBuffer, note: &Note, channels: u16) -> Vec<f32> {
    let pitch_shift = pitch::pitch_shift_for(sample, note);
    // combined ratio: pitch-shift + conversion from the .wav's native
    // sample rate to the output sample rate.
    let rate_ratio = source.sample_rate as f64 / OUTPUT_SAMPLE_RATE as f64;
    let resampled = dsp::resample_multi(
        &source.data,
        pitch_shift.ratio * rate_ratio,
        source.channels() as usize,
    );
    dsp::upmix(&resampled, source.channels() as usize, channels as usize)
}

/// Renders a single pattern on its own, once, from its first step (for
/// auditioning it), mono. The same as [`render_pattern_multi`] with
/// `channels: 1`.
pub fn render_pattern(
    pattern: &Pattern,
    samples: &[Sample],
    audio: &HashMap<String, AudioBuffer>,
    seconds_per_step: f64,
) -> Option<AudioBuffer> {
    render_pattern_multi(pattern, samples, audio, seconds_per_step, 1)
}

/// The same as [`render_pattern`], at `channels` channels (see
/// [`render_tracks_multi`] for how a narrower source is upmixed). `None`
/// if its sample is unknown or not in `audio`. Steps that are not valid
/// notes are skipped (validation rejects them before a composition gets
/// this far).
pub fn render_pattern_multi(
    pattern: &Pattern,
    samples: &[Sample],
    audio: &HashMap<String, AudioBuffer>,
    seconds_per_step: f64,
    channels: u16,
) -> Option<AudioBuffer> {
    let channels = channels.max(1);
    let sample = samples.iter().find(|s| s.id == pattern.sample)?;
    let source = audio.get(&pattern.sample)?;
    let frames_per_step = seconds_per_step * OUTPUT_SAMPLE_RATE as f64;

    let mut buffer: Vec<f32> = Vec::new();
    for (index, raw) in pattern.steps.iter().enumerate() {
        let Some(raw) = raw else { continue };
        let Ok(note) = parse_note(raw) else { continue };
        let voice = render_voice(sample, source, &note, channels);
        let start = (index as f64 * frames_per_step).round() as usize;
        dsp::mix_into_multi(&mut buffer, &voice, start, channels as usize);
    }
    dsp::normalize(&mut buffer);
    Some(AudioBuffer::new_multi(buffer, OUTPUT_SAMPLE_RATE, channels))
}

/// Sums the tracks into a single buffer, skipping the ones flagged in
/// `muted` (missing entries count as not muted), and normalizes the
/// result if its peak exceeds `1.0`. Works at whatever channel count the
/// tracks themselves share (they must all share one — see
/// [`render_tracks_multi`]): summing an interleaved buffer position by
/// position, from the start, is already channel-count agnostic.
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
    fn render_tracks_multi_with_one_channel_matches_render_tracks() {
        let composition = parse_composition(SONG).unwrap();
        let arrangement = resolve(&composition);
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 10], OUTPUT_SAMPLE_RATE));
        let sps = seconds_per_step(&composition.metadata);

        let plain = render_tracks(&arrangement, &composition.samples, &audio, sps);
        let via_multi = render_tracks_multi(&arrangement, &composition.samples, &audio, sps, 1);
        assert_eq!(plain.len(), via_multi.len());
        for (a, b) in plain.iter().zip(&via_multi) {
            assert_eq!(a.track_id, b.track_id);
            assert_eq!(a.audio.data, b.audio.data);
            assert_eq!(b.audio.channels(), 1);
        }
    }

    #[test]
    fn a_mono_voice_is_upmixed_to_the_track_s_channel_count() {
        let composition = parse_composition(SONG).unwrap();
        let arrangement = resolve(&composition);
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 10], OUTPUT_SAMPLE_RATE)); // mono
        let sps = seconds_per_step(&composition.metadata);

        let tracks = render_tracks_multi(&arrangement, &composition.samples, &audio, sps, 2);
        assert_eq!(tracks[0].audio.channels(), 2);
        // Frame 0 (samples 0 and 1) is the mono 0.5 duplicated to L and R.
        assert_eq!(&tracks[0].audio.data[0..2], &[0.5, 0.5]);
    }

    #[test]
    fn a_stereo_voice_keeps_its_channels_distinct_through_rendering() {
        let composition = parse_composition(SONG).unwrap();
        let arrangement = resolve(&composition);
        let mut audio = HashMap::new();
        // Stereo source: left is 1.0, right is 0.0, every frame.
        let stereo_data: Vec<f32> = (0..10).flat_map(|_| [1.0, 0.0]).collect();
        audio.insert("s".to_string(), AudioBuffer::new_multi(stereo_data, OUTPUT_SAMPLE_RATE, 2));
        let sps = seconds_per_step(&composition.metadata);

        let tracks = render_tracks_multi(&arrangement, &composition.samples, &audio, sps, 2);
        assert_eq!(tracks[0].audio.channels(), 2);
        assert_eq!(&tracks[0].audio.data[0..2], &[1.0, 0.0]);
    }

    #[test]
    fn mono_and_stereo_tracks_mix_together_once_both_are_upmixed() {
        let composition = parse_composition(SONG).unwrap();
        let arrangement = resolve(&composition);
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 10], OUTPUT_SAMPLE_RATE)); // mono
        let sps = seconds_per_step(&composition.metadata);

        // Both of the song's tracks use the same (mono) sample here, but
        // rendered at channels: 2 — this is exactly what a real mixed
        // mono+stereo composition looks like once render_tracks_multi has
        // upmixed the mono one: every TrackBuffer ends up at the same
        // (wider) channel count, so mix_tracks can sum them unchanged.
        let tracks = render_tracks_multi(&arrangement, &composition.samples, &audio, sps, 2);
        let master = mix_tracks(&tracks, &[]);
        // Two identical upmixed 0.5 voices sum to 1.0 on both channels.
        assert_eq!(&master[0..2], &[1.0, 1.0]);
    }

    #[test]
    fn native_channels_reports_each_track_s_own_width_even_once_homogenized() {
        // Two tracks, two different samples: "m" (mono) on track a, "s"
        // (stereo) on track b. Both get rendered (upmixed where needed) to
        // the same channels: 2, as any mixed composition's tracks are, but
        // native_channels must still tell them apart.
        const TWO_TRACKS: &str = r#"{
            "metadata": { "version": "1.0", "title": "t", "bpm": 120 },
            "samples": [ { "id": "m", "file": "m.wav" }, { "id": "s", "file": "s.wav" } ],
            "patterns": [
                { "id": "pm", "sample": "m", "steps": ["C4", null, null, null] },
                { "id": "ps", "sample": "s", "steps": ["C4", null, null, null] }
            ],
            "arrangement": { "tracks": [
                { "id": "a", "sequence": ["pm"] },
                { "id": "b", "sequence": ["ps"] }
            ] }
        }"#;
        let composition = parse_composition(TWO_TRACKS).unwrap();
        let arrangement = resolve(&composition);
        let mut audio = HashMap::new();
        audio.insert("m".to_string(), AudioBuffer::new(vec![0.5; 10], OUTPUT_SAMPLE_RATE));
        let stereo_data: Vec<f32> = (0..5).flat_map(|_| [1.0, 0.0]).collect();
        audio.insert("s".to_string(), AudioBuffer::new_multi(stereo_data, OUTPUT_SAMPLE_RATE, 2));
        let sps = seconds_per_step(&composition.metadata);

        let tracks = render_tracks_multi(&arrangement, &composition.samples, &audio, sps, 2);
        // Both buffers are at the shared width...
        assert_eq!(tracks[0].audio.channels(), 2);
        assert_eq!(tracks[1].audio.channels(), 2);
        // ...but each track still reports what it actually, natively is.
        assert_eq!(tracks[0].native_channels, 1);
        assert_eq!(tracks[1].native_channels, 2);
    }

    #[test]
    fn a_silent_track_reports_one_native_channel() {
        let composition = parse_composition(SONG).unwrap();
        let arrangement = resolve(&composition);
        // Clear every step so both tracks play nothing at all: with no
        // sample ever looked at, native_channels must still fall back to a
        // sane default (1) instead of panicking or staying uninitialized.
        let mut empty_arrangement = arrangement.clone();
        for track in &mut empty_arrangement.tracks {
            for step in &mut track.steps {
                *step = None;
            }
        }
        let audio = HashMap::new();
        let sps = seconds_per_step(&composition.metadata);
        let tracks = render_tracks_multi(&empty_arrangement, &composition.samples, &audio, sps, 2);
        assert_eq!(tracks[0].native_channels, 1);
    }

    #[test]
    fn render_pattern_multi_with_one_channel_matches_render_pattern() {
        let c = parse_composition(SONG).unwrap();
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 100], 44100));
        let sps = 0.01;

        let plain = render_pattern(&c.patterns[0], &c.samples, &audio, sps).unwrap();
        let via_multi = render_pattern_multi(&c.patterns[0], &c.samples, &audio, sps, 1).unwrap();
        assert_eq!(plain.data, via_multi.data);
        assert_eq!(via_multi.channels(), 1);
    }

    #[test]
    fn render_pattern_multi_upmixes_a_mono_sample() {
        let c = parse_composition(SONG).unwrap();
        let mut audio = HashMap::new();
        audio.insert("s".to_string(), AudioBuffer::new(vec![0.5; 100], 44100));
        let buf = render_pattern_multi(&c.patterns[0], &c.samples, &audio, 0.01, 2).unwrap();
        assert_eq!(buf.channels(), 2);
        assert!(buf.data.iter().step_by(2).zip(buf.data.iter().skip(1).step_by(2)).all(|(l, r)| l == r));
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
