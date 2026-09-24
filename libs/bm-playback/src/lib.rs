//! Audio output through the system's default device (WASAPI on Windows,
//! ALSA on Linux — `cpal` abstracts which one).
//!
//! [`Engine`] is non-blocking: create it from already-rendered per-track
//! buffers, then control it from any thread (play, pause, stop, seek, loop,
//! mute per track, master volume, one-shot sample previews) and read its
//! position. The audio callback is real-time
//! safe: it takes no locks and does no allocation — all shared state is
//! atomics — so the UI thread can never make it glitch.
//!
//! This is the only bit-music crate that depends on `cpal`; everything else
//! builds without system audio libraries.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use bm_dsp::{self as dsp, AudioBuffer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("no audio output device found")]
    NoOutputDevice,
    #[error("could not get the device's default config: {0}")]
    DefaultConfig(#[from] cpal::DefaultStreamConfigError),
    #[error("unsupported device sample format: {0:?}")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("could not build the audio stream: {0}")]
    BuildStream(#[from] cpal::BuildStreamError),
    #[error("could not start the audio stream: {0}")]
    PlayStream(#[from] cpal::PlayStreamError),
}

/// Sentinel meaning "no seek requested".
const NO_SEEK: usize = usize::MAX;

/// Sentinel meaning "this preview is not playing".
const IDLE: usize = usize::MAX;

/// Length of the window shown by the scope methods, in seconds (half of it
/// on each side of the current position).
const SCOPE_SECONDS: f64 = 0.12;

/// The audio around `center`, reduced to `points` values for drawing: each
/// value is the sample of largest magnitude in its share of the window
/// (`half` frames on each side of `center`), so peaks are not lost. Frames
/// outside `data` count as silence.
pub fn scope_window(data: &[f32], center: usize, half: usize, points: usize) -> Vec<f32> {
    let total = half * 2;
    let start = center as i64 - half as i64;
    (0..points)
        .map(|p| {
            let lo = start + (p * total / points.max(1)) as i64;
            let hi = start + ((p + 1) * total / points.max(1)) as i64;
            let mut best = 0.0f32;
            for i in lo..hi.max(lo + 1) {
                let v = usize::try_from(i).ok().and_then(|i| data.get(i)).copied().unwrap_or(0.0);
                if v.abs() > best.abs() {
                    best = v;
                }
            }
            best
        })
        .collect()
}

/// State shared between the controlling thread and the audio callback.
/// Everything mutable is atomic; the audio data is immutable.
struct Core {
    /// One mono buffer per track, already at the device sample rate.
    tracks: Vec<Vec<f32>>,
    muted: Vec<AtomicBool>,
    /// Short one-shot sounds (sample previews), already at the device rate.
    previews: Vec<Vec<f32>>,
    /// Next frame of each preview, or `IDLE`.
    preview_pos: Vec<AtomicUsize>,
    /// Master volume as `f32` bits (1.0 = unchanged).
    volume: AtomicU32,
    /// Length in frames of the longest track.
    len: usize,
    /// Fixed gain that keeps the full (unmuted) mix from clipping.
    gain: f32,
    /// Next frame to play. Written only by the audio callback.
    position: AtomicUsize,
    /// Pending seek target (frames), or `NO_SEEK`. Written by the
    /// controlling thread, consumed by the callback.
    seek_request: AtomicUsize,
    playing: AtomicBool,
    looping: AtomicBool,
    finished: AtomicBool,
    stream_error: AtomicBool,
}

impl Core {
    fn new(tracks: Vec<Vec<f32>>, previews: Vec<Vec<f32>>) -> Self {
        let len = tracks.iter().map(Vec::len).max().unwrap_or(0);

        // Same normalization the offline mix uses: one fixed gain from the
        // peak of the full mix, so nothing clips when all tracks play.
        let mut full_mix = Vec::new();
        for track in &tracks {
            dsp::mix_into(&mut full_mix, track, 0);
        }
        let gain = dsp::normalization_gain(dsp::peak(&full_mix));

        let muted = tracks.iter().map(|_| AtomicBool::new(false)).collect();
        let preview_pos = previews.iter().map(|_| AtomicUsize::new(IDLE)).collect();
        Self {
            tracks,
            muted,
            previews,
            preview_pos,
            volume: AtomicU32::new(1.0f32.to_bits()),
            len,
            gain,
            position: AtomicUsize::new(0),
            seek_request: AtomicUsize::new(NO_SEEK),
            playing: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            stream_error: AtomicBool::new(false),
        }
    }

    /// Fills an interleaved output block. Called from the audio callback:
    /// no locks, no allocation.
    fn fill<T: Sample + FromSample<f32>>(&self, out: &mut [T], channels: usize) {
        let requested = self.seek_request.swap(NO_SEEK, Ordering::Relaxed);
        let mut pos = if requested != NO_SEEK {
            requested.min(self.len)
        } else {
            self.position.load(Ordering::Relaxed)
        };
        let mut playing = self.playing.load(Ordering::Relaxed);
        let looping = self.looping.load(Ordering::Relaxed);
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));

        for frame in out.chunks_mut(channels.max(1)) {
            let mut value = 0.0f32;

            if playing && pos < self.len {
                for (i, track) in self.tracks.iter().enumerate() {
                    if !self.muted[i].load(Ordering::Relaxed) {
                        value += track.get(pos).copied().unwrap_or(0.0);
                    }
                }
                value *= self.gain;
                pos += 1;
            }

            if playing && pos >= self.len {
                if looping && self.len > 0 {
                    pos = 0;
                } else {
                    playing = false;
                    self.playing.store(false, Ordering::Relaxed);
                    self.finished.store(true, Ordering::Relaxed);
                }
            }

            // Sample previews sound on top of the transport, even when paused.
            for (buf, pos) in self.previews.iter().zip(&self.preview_pos) {
                let p = pos.load(Ordering::Relaxed);
                if p != IDLE {
                    value += buf.get(p).copied().unwrap_or(0.0);
                    pos.store(if p + 1 < buf.len() { p + 1 } else { IDLE }, Ordering::Relaxed);
                }
            }

            let value = (value * volume).clamp(-1.0, 1.0);
            for sample in frame.iter_mut() {
                *sample = T::from_sample(value);
            }
        }

        self.position.store(pos, Ordering::Relaxed);
    }

    fn preview_active(&self, index: usize) -> bool {
        self.preview_pos
            .get(index)
            .is_some_and(|p| p.load(Ordering::Relaxed) != IDLE)
    }

    fn current_position(&self) -> usize {
        let requested = self.seek_request.load(Ordering::Relaxed);
        if requested != NO_SEEK {
            requested.min(self.len)
        } else {
            self.position.load(Ordering::Relaxed)
        }
    }
}

/// A non-blocking playback engine over a set of per-track buffers.
///
/// Not `Send`: keep it on the thread that created it (the underlying
/// `cpal` stream is not `Send` on every platform).
pub struct Engine {
    core: Arc<Core>,
    device_sample_rate: u32,
    _stream: cpal::Stream,
}

impl Engine {
    /// Opens the default output device and prepares `tracks` for playback.
    /// The stream starts running immediately but outputs silence until
    /// [`play`](Self::play) is called.
    pub fn new<'a>(
        tracks: impl IntoIterator<Item = &'a AudioBuffer>,
    ) -> Result<Self, PlaybackError> {
        Self::with_previews(tracks, std::iter::empty())
    }

    /// Like [`new`](Self::new), plus a set of one-shot sounds that can be
    /// triggered by index with [`play_preview`](Self::play_preview).
    pub fn with_previews<'a>(
        tracks: impl IntoIterator<Item = &'a AudioBuffer>,
        previews: impl IntoIterator<Item = &'a AudioBuffer>,
    ) -> Result<Self, PlaybackError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(PlaybackError::NoOutputDevice)?;

        let supported = device.default_output_config()?;
        let sample_format = supported.sample_format();
        let channels = supported.channels() as usize;
        let device_sample_rate = supported.sample_rate().0;
        let stream_config: StreamConfig = supported.config();

        // Bring every buffer to the device sample rate up front (no pitch
        // change: same resampling math, ratio = source rate / device rate).
        let to_device = |t: &AudioBuffer| -> Vec<f32> {
            if t.sample_rate == device_sample_rate {
                t.data.clone()
            } else {
                dsp::resample(&t.data, t.sample_rate as f64 / device_sample_rate as f64)
            }
        };
        let device_tracks: Vec<Vec<f32>> = tracks.into_iter().map(to_device).collect();
        let device_previews: Vec<Vec<f32>> = previews.into_iter().map(to_device).collect();

        let core = Arc::new(Core::new(device_tracks, device_previews));

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, channels, &core)?,
            SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, channels, &core)?,
            SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, channels, &core)?,
            other => return Err(PlaybackError::UnsupportedSampleFormat(other)),
        };
        stream.play()?;

        Ok(Self {
            core,
            device_sample_rate,
            _stream: stream,
        })
    }

    /// Starts (or resumes) playback. If the previous run reached the end,
    /// starts again from the beginning.
    pub fn play(&self) {
        if self.core.finished.swap(false, Ordering::Relaxed) {
            self.core.seek_request.store(0, Ordering::Relaxed);
        }
        self.core.playing.store(true, Ordering::Relaxed);
    }

    /// Pauses playback, keeping the position.
    pub fn pause(&self) {
        self.core.playing.store(false, Ordering::Relaxed);
    }

    /// Stops playback and rewinds to the start.
    pub fn stop(&self) {
        self.core.playing.store(false, Ordering::Relaxed);
        self.core.finished.store(false, Ordering::Relaxed);
        self.core.seek_request.store(0, Ordering::Relaxed);
    }

    /// Jumps to `seconds` (clamped to the song length).
    pub fn seek_seconds(&self, seconds: f64) {
        let frame = (seconds.max(0.0) * self.device_sample_rate as f64).round() as usize;
        self.core
            .seek_request
            .store(frame.min(self.core.len), Ordering::Relaxed);
        self.core.finished.store(false, Ordering::Relaxed);
    }

    /// Sets the master volume (clamped to 0.0..=1.0), applied to the
    /// transport and to previews.
    pub fn set_volume(&self, volume: f32) {
        self.core
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.core.volume.load(Ordering::Relaxed))
    }

    /// Plays preview `index` from its start, on top of whatever else is
    /// sounding and without touching the transport. Restarts it if it is
    /// already sounding; ignored if out of range.
    pub fn play_preview(&self, index: usize) {
        if let Some(pos) = self.core.preview_pos.get(index) {
            pos.store(0, Ordering::Relaxed);
        }
    }

    pub fn preview_count(&self) -> usize {
        self.core.previews.len()
    }

    /// `true` while preview `index` is still sounding (so a UI can keep
    /// its play button disabled until it ends).
    pub fn is_preview_playing(&self, index: usize) -> bool {
        self.core.preview_active(index)
    }

    /// `true` while any preview is sounding.
    pub fn any_preview_playing(&self) -> bool {
        (0..self.core.previews.len()).any(|i| self.core.preview_active(i))
    }

    /// What track `index` is playing around the current position, as
    /// `points` values for drawing an oscilloscope (scaled like the output:
    /// with the mix gain and the master volume). Empty while nothing is
    /// playing or when the track is muted.
    pub fn track_scope(&self, index: usize, points: usize) -> Vec<f32> {
        if !self.is_playing() || self.is_muted(index) {
            return Vec::new();
        }
        let Some(track) = self.core.tracks.get(index) else {
            return Vec::new();
        };
        let scale = self.core.gain * self.volume();
        scope_window(track, self.core.current_position(), self.scope_half(), points)
            .into_iter()
            .map(|v| v * scale)
            .collect()
    }

    /// The same for preview `index`: empty unless it is sounding.
    pub fn preview_scope(&self, index: usize, points: usize) -> Vec<f32> {
        let Some(pos) = self.core.preview_pos.get(index) else {
            return Vec::new();
        };
        let position = pos.load(Ordering::Relaxed);
        if position == IDLE {
            return Vec::new();
        }
        let volume = self.volume();
        scope_window(&self.core.previews[index], position, self.scope_half(), points)
            .into_iter()
            .map(|v| v * volume)
            .collect()
    }

    fn scope_half(&self) -> usize {
        (self.device_sample_rate as f64 * SCOPE_SECONDS / 2.0) as usize
    }

    pub fn set_looping(&self, looping: bool) {
        self.core.looping.store(looping, Ordering::Relaxed);
    }

    pub fn is_looping(&self) -> bool {
        self.core.looping.load(Ordering::Relaxed)
    }

    /// Mutes or unmutes track `index` (ignored if out of range).
    pub fn set_muted(&self, index: usize, muted: bool) {
        if let Some(flag) = self.core.muted.get(index) {
            flag.store(muted, Ordering::Relaxed);
        }
    }

    pub fn is_muted(&self, index: usize) -> bool {
        self.core
            .muted
            .get(index)
            .is_some_and(|f| f.load(Ordering::Relaxed))
    }

    pub fn is_playing(&self) -> bool {
        self.core.playing.load(Ordering::Relaxed)
    }

    /// `true` once a non-looping run has reached the end.
    pub fn has_finished(&self) -> bool {
        self.core.finished.load(Ordering::Relaxed)
    }

    /// `true` if the audio stream reported an error (e.g. device unplugged).
    pub fn has_stream_error(&self) -> bool {
        self.core.stream_error.load(Ordering::Relaxed)
    }

    pub fn track_count(&self) -> usize {
        self.core.tracks.len()
    }

    pub fn position_seconds(&self) -> f64 {
        self.core.current_position() as f64 / self.device_sample_rate as f64
    }

    pub fn duration_seconds(&self) -> f64 {
        self.core.len as f64 / self.device_sample_rate as f64
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    core: &Arc<Core>,
) -> Result<cpal::Stream, PlaybackError>
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let data_core = Arc::clone(core);
    let err_core = Arc::clone(core);

    let stream = device.build_output_stream(
        config,
        move |output: &mut [T], _info| data_core.fill(output, channels),
        move |_err| err_core.stream_error.store(true, Ordering::Relaxed),
        None,
    )?;

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core(tracks: Vec<Vec<f32>>) -> Core {
        Core::new(tracks, Vec::new())
    }

    /// Runs one callback block over `frames` frames of `channels` channels.
    fn block(core: &Core, frames: usize, channels: usize) -> Vec<f32> {
        let mut out = vec![9.0f32; frames * channels];
        core.fill(&mut out, channels);
        out
    }

    #[test]
    fn paused_engine_outputs_silence_and_does_not_advance() {
        let c = core(vec![vec![0.5; 8]]);
        assert!(block(&c, 4, 2).iter().all(|&v| v == 0.0));
        assert_eq!(c.current_position(), 0);
    }

    #[test]
    fn playing_sums_tracks_and_replicates_mono_to_every_channel() {
        let c = core(vec![vec![0.25; 8], vec![0.25; 8]]);
        c.playing.store(true, Ordering::Relaxed);
        let out = block(&c, 2, 2);
        assert_eq!(out, vec![0.5, 0.5, 0.5, 0.5]);
        assert_eq!(c.current_position(), 2);
    }

    #[test]
    fn muted_tracks_are_left_out() {
        let c = core(vec![vec![0.25; 4], vec![0.5; 4]]);
        c.playing.store(true, Ordering::Relaxed);
        c.muted[1].store(true, Ordering::Relaxed);
        assert_eq!(block(&c, 1, 1), vec![0.25]);
    }

    #[test]
    fn full_mix_that_would_clip_is_scaled_by_a_fixed_gain() {
        // two 1.0 tracks -> peak 2.0 -> gain 0.5 -> output 1.0
        let c = core(vec![vec![1.0; 4], vec![1.0; 4]]);
        c.playing.store(true, Ordering::Relaxed);
        assert_eq!(block(&c, 1, 1), vec![1.0]);
    }

    #[test]
    fn reaching_the_end_stops_and_marks_finished() {
        let c = core(vec![vec![0.5; 2]]);
        c.playing.store(true, Ordering::Relaxed);
        let out = block(&c, 4, 1);
        assert_eq!(out, vec![0.5, 0.5, 0.0, 0.0]);
        assert!(!c.playing.load(Ordering::Relaxed));
        assert!(c.finished.load(Ordering::Relaxed));
    }

    #[test]
    fn looping_wraps_around_instead_of_finishing() {
        let c = core(vec![vec![0.1, 0.2]]);
        c.playing.store(true, Ordering::Relaxed);
        c.looping.store(true, Ordering::Relaxed);
        assert_eq!(block(&c, 5, 1), vec![0.1, 0.2, 0.1, 0.2, 0.1]);
        assert!(c.playing.load(Ordering::Relaxed));
        assert!(!c.finished.load(Ordering::Relaxed));
    }

    #[test]
    fn seek_takes_effect_on_the_next_block() {
        let c = core(vec![vec![0.1, 0.2, 0.3, 0.4]]);
        c.playing.store(true, Ordering::Relaxed);
        c.seek_request.store(2, Ordering::Relaxed);
        // position already reflects the pending seek
        assert_eq!(c.current_position(), 2);
        assert_eq!(block(&c, 2, 1), vec![0.3, 0.4]);
    }

    #[test]
    fn the_scope_window_keeps_peaks_and_pads_with_silence() {
        let data = [0.0, 0.5, -0.9, 0.2, 0.0, 0.0];
        // 4 frames each side of frame 2: frames -2..6, in 4 points of 2 frames.
        let w = scope_window(&data, 2, 4, 4);
        assert_eq!(w.len(), 4);
        assert_eq!(w[0], 0.0); // frames -2, -1: before the start
        assert_eq!(w[1], 0.5); // frames 0, 1
        assert_eq!(w[2], -0.9); // frames 2, 3: the largest magnitude keeps its sign
        assert_eq!(w[3], 0.0); // frames 4, 5
        assert!(scope_window(&[], 0, 10, 8).iter().all(|v| *v == 0.0));
        assert!(scope_window(&data, 2, 4, 0).is_empty());
    }

    #[test]
    fn volume_scales_the_output() {
        let c = core(vec![vec![0.5; 4]]);
        c.playing.store(true, Ordering::Relaxed);
        c.volume.store(0.5f32.to_bits(), Ordering::Relaxed);
        assert_eq!(block(&c, 1, 1), vec![0.25]);
    }

    #[test]
    fn a_preview_sounds_once_while_the_transport_is_paused() {
        let c = Core::new(vec![vec![0.0; 8]], vec![vec![0.1, 0.2]]);
        // Idle until triggered.
        assert_eq!(block(&c, 2, 1), vec![0.0, 0.0]);
        c.preview_pos[0].store(0, Ordering::Relaxed);
        assert!(c.preview_active(0));
        assert_eq!(block(&c, 1, 1), vec![0.1]);
        assert!(c.preview_active(0)); // still sounding: one frame left
        assert_eq!(block(&c, 3, 1), vec![0.2, 0.0, 0.0]);
        assert_eq!(c.preview_pos[0].load(Ordering::Relaxed), IDLE);
        assert!(!c.preview_active(0));
        // The transport did not move.
        assert_eq!(c.current_position(), 0);
    }

    #[test]
    fn a_preview_is_mixed_with_the_transport_and_never_clips() {
        let c = Core::new(vec![vec![0.5; 2]], vec![vec![0.75; 2]]);
        c.playing.store(true, Ordering::Relaxed);
        c.preview_pos[0].store(0, Ordering::Relaxed);
        assert_eq!(block(&c, 1, 1), vec![1.0]);
    }
}
