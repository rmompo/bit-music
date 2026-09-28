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

/// Ceiling on how many channels a mix can have, so [`Core::fill`] (the
/// real-time audio callback) can accumulate a frame in a fixed-size stack
/// array instead of allocating. Nothing in bit-music produces more than 2
/// today; this leaves generous headroom without ever allocating.
const MAX_MIX_CHANNELS: usize = 8;

/// The value for one device output channel, given a frame already mixed
/// at `mix_frame.len()` channels: passes each mix channel straight
/// through when the device has at least as many; averages every mix
/// channel together when it does not (e.g. a stereo mix on a mono
/// device); repeats the one channel of a mono mix everywhere (today's
/// behavior, and the only case when `mix_frame.len() == 1`).
fn device_channel_value(mix_frame: &[f32], device_channels: usize, device_ch: usize) -> f32 {
    let mix_channels = mix_frame.len();
    if mix_channels <= 1 {
        mix_frame.first().copied().unwrap_or(0.0)
    } else if device_channels >= mix_channels {
        mix_frame.get(device_ch).copied().unwrap_or(0.0)
    } else {
        mix_frame.iter().sum::<f32>() / mix_channels as f32
    }
}

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

/// One [`scope_window`] per channel of interleaved `data` (`mix_channels`
/// channels), each scaled by `gain`; a mono buffer (`mix_channels: 1`)
/// gives exactly one, so this is the multi-channel counterpart callers
/// widen a single-channel scope into.
pub fn scope_channels(
    data: &[f32],
    mix_channels: usize,
    center: usize,
    half: usize,
    points: usize,
    gain: f32,
) -> Vec<Vec<f32>> {
    let mix_channels = mix_channels.max(1);
    (0..mix_channels)
        .map(|ch| {
            let channel_data: Vec<f32> = data.iter().skip(ch).step_by(mix_channels).copied().collect();
            scope_window(&channel_data, center, half, points).into_iter().map(|v| v * gain).collect()
        })
        .collect()
}

/// State shared between the controlling thread and the audio callback.
/// Everything mutable is atomic; the audio data is immutable.
struct Core {
    /// One interleaved buffer per track, [`mix_channels`](Self::mix_channels)
    /// channels each, already at the device sample rate.
    tracks: Vec<Vec<f32>>,
    /// Each track's own channel count before it was upmixed to
    /// `mix_channels` to fit alongside the others (parallel to `tracks`).
    /// A scope truncates to this so a mono track shows one trace, not
    /// `mix_channels` identical copies of it.
    track_channels: Vec<usize>,
    /// Every track and preview buffer is at this many channels (the widest
    /// among them all — see [`Engine::with_previews`], which upmixes a
    /// narrower one to match): `1` behaves exactly as before.
    mix_channels: usize,
    muted: Vec<AtomicBool>,
    /// Short one-shot sounds (sample previews), already at the device rate
    /// and at `mix_channels` channels.
    previews: Vec<Vec<f32>>,
    /// Each preview's own channel count before upmixing (parallel to
    /// `previews`) — see `track_channels`.
    preview_channels: Vec<usize>,
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
    /// `tracks` and `previews` must already be interleaved at
    /// `mix_channels` channels each (narrower buffers upmixed by the
    /// caller — see [`Engine::with_previews`]); each pairs that data with
    /// its buffer's own native channel count from before the upmix, kept
    /// only so a scope can show the right number of traces.
    fn new(tracks: Vec<(Vec<f32>, usize)>, previews: Vec<(Vec<f32>, usize)>, mix_channels: usize) -> Self {
        let mix_channels = mix_channels.clamp(1, MAX_MIX_CHANNELS);
        let (tracks, track_channels): (Vec<Vec<f32>>, Vec<usize>) = tracks.into_iter().unzip();
        let (previews, preview_channels): (Vec<Vec<f32>>, Vec<usize>) = previews.into_iter().unzip();
        let len = tracks.iter().map(|t| t.len() / mix_channels).max().unwrap_or(0);

        // Same normalization the offline mix uses: one fixed gain from the
        // peak of the full mix, so nothing clips when all tracks play.
        // mix_into sums an interleaved buffer position by position, which
        // is already channel-count agnostic as long as every track shares
        // one — true here by construction.
        let mut full_mix = Vec::new();
        for track in &tracks {
            dsp::mix_into(&mut full_mix, track, 0);
        }
        let gain = dsp::normalization_gain(dsp::peak(&full_mix));

        let muted = tracks.iter().map(|_| AtomicBool::new(false)).collect();
        let preview_pos = previews.iter().map(|_| AtomicUsize::new(IDLE)).collect();
        Self {
            tracks,
            track_channels,
            mix_channels,
            muted,
            previews,
            preview_channels,
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

    /// Fills an interleaved output block of `device_channels` channels.
    /// Called from the audio callback: no locks, no allocation (the
    /// per-frame mix accumulates in a fixed-size array — see
    /// [`MAX_MIX_CHANNELS`] — never a `Vec`).
    fn fill<T: Sample + FromSample<f32>>(&self, out: &mut [T], device_channels: usize) {
        let requested = self.seek_request.swap(NO_SEEK, Ordering::Relaxed);
        let mut pos = if requested != NO_SEEK {
            requested.min(self.len)
        } else {
            self.position.load(Ordering::Relaxed)
        };
        let mut playing = self.playing.load(Ordering::Relaxed);
        let looping = self.looping.load(Ordering::Relaxed);
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let mix_channels = self.mix_channels;
        let device_channels = device_channels.max(1);

        for frame in out.chunks_mut(device_channels) {
            let mut mix_frame = [0.0f32; MAX_MIX_CHANNELS];
            let mix_frame = &mut mix_frame[..mix_channels];

            if playing && pos < self.len {
                for (i, track) in self.tracks.iter().enumerate() {
                    if !self.muted[i].load(Ordering::Relaxed) {
                        for (ch, slot) in mix_frame.iter_mut().enumerate() {
                            *slot += track.get(pos * mix_channels + ch).copied().unwrap_or(0.0);
                        }
                    }
                }
                for slot in mix_frame.iter_mut() {
                    *slot *= self.gain;
                }
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
            for (buf, preview_pos) in self.previews.iter().zip(&self.preview_pos) {
                let p = preview_pos.load(Ordering::Relaxed);
                if p != IDLE {
                    for (ch, slot) in mix_frame.iter_mut().enumerate() {
                        *slot += buf.get(p * mix_channels + ch).copied().unwrap_or(0.0);
                    }
                    let preview_frames = buf.len() / mix_channels;
                    preview_pos.store(if p + 1 < preview_frames { p + 1 } else { IDLE }, Ordering::Relaxed);
                }
            }

            for slot in mix_frame.iter_mut() {
                *slot = (*slot * volume).clamp(-1.0, 1.0);
            }

            for (device_ch, sample) in frame.iter_mut().enumerate() {
                *sample = T::from_sample(device_channel_value(mix_frame, device_channels, device_ch));
            }
        }

        self.position.store(pos, Ordering::Relaxed);
    }

    /// One [`scope_window`] per *native* channel of track `index` (see
    /// [`track_channels`](Self::track_channels)), centered at `position` —
    /// a mono track shows one trace even though it was upmixed to
    /// `mix_channels` internally to sit alongside a wider one. Empty if
    /// `index` is out of range.
    fn track_scope_multi(&self, index: usize, position: usize, half: usize, points: usize) -> Vec<Vec<f32>> {
        let Some(track) = self.tracks.get(index) else {
            return Vec::new();
        };
        let native = self.track_channels.get(index).copied().unwrap_or(self.mix_channels);
        let mut channels = scope_channels(track, self.mix_channels, position, half, points, self.gain);
        channels.truncate(native);
        channels
    }

    /// The same as [`track_scope_multi`](Self::track_scope_multi), for
    /// preview `index`.
    fn preview_scope_multi(&self, index: usize, position: usize, half: usize, points: usize) -> Vec<Vec<f32>> {
        let Some(preview) = self.previews.get(index) else {
            return Vec::new();
        };
        let native = self.preview_channels.get(index).copied().unwrap_or(self.mix_channels);
        let mut channels = scope_channels(preview, self.mix_channels, position, half, points, 1.0);
        channels.truncate(native);
        channels
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
        let device_channels = supported.channels() as usize;
        let device_sample_rate = supported.sample_rate().0;
        let stream_config: StreamConfig = supported.config();

        let tracks: Vec<&AudioBuffer> = tracks.into_iter().collect();
        let previews: Vec<&AudioBuffer> = previews.into_iter().collect();
        // The widest channel count among everything that can sound (a
        // track or a preview) — every buffer below is upmixed to this, so
        // Core can index them all the same way. `1` (nothing but mono
        // buffers) reproduces today's behavior exactly.
        let mix_channels = tracks
            .iter()
            .chain(&previews)
            .map(|t| t.channels() as usize)
            .max()
            .unwrap_or(1);

        // Brings `t` to the device sample rate (no pitch change: same
        // resampling math, ratio = source rate / device rate — done per
        // channel, at `t`'s own channel count, so channels are never
        // blended together), then up to `mix_channels` channels.
        let to_device = |t: &AudioBuffer| -> Vec<f32> {
            let resampled = if t.sample_rate == device_sample_rate {
                t.data.clone()
            } else {
                dsp::resample_multi(
                    &t.data,
                    t.sample_rate as f64 / device_sample_rate as f64,
                    t.channels() as usize,
                )
            };
            dsp::upmix(&resampled, t.channels() as usize, mix_channels)
        };
        let device_tracks: Vec<(Vec<f32>, usize)> =
            tracks.iter().map(|t| (to_device(t), t.channels() as usize)).collect();
        let device_previews: Vec<(Vec<f32>, usize)> =
            previews.iter().map(|t| (to_device(t), t.channels() as usize)).collect();

        let core = Arc::new(Core::new(device_tracks, device_previews, mix_channels));

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, device_channels, &core)?,
            SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, device_channels, &core)?,
            SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, device_channels, &core)?,
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
    /// `points` values for drawing an oscilloscope — its first channel
    /// only (identical to today for a mono track; see
    /// [`track_scope_multi`](Self::track_scope_multi) for every channel of
    /// a multi-channel one). Scaled by the mix gain only — *not* the
    /// master volume, so turning the app's volume down never shrinks the
    /// trace. Empty while nothing is playing or when the track is muted.
    pub fn track_scope(&self, index: usize, points: usize) -> Vec<f32> {
        self.track_scope_multi(index, points).into_iter().next().unwrap_or_default()
    }

    /// The same as [`track_scope`](Self::track_scope), but every channel
    /// (one inner `Vec` per channel, in source order; a mono track gives
    /// exactly one, matching `track_scope` — even when it plays alongside
    /// a wider track and was upmixed to fit the shared mix internally, its
    /// scope still shows one trace, not several identical copies of it).
    pub fn track_scope_multi(&self, index: usize, points: usize) -> Vec<Vec<f32>> {
        if !self.is_playing() || self.is_muted(index) {
            return Vec::new();
        }
        self.core
            .track_scope_multi(index, self.core.current_position(), self.scope_half(), points)
    }

    /// The same for preview `index`: empty unless it is sounding. Not
    /// scaled by the master volume either, for the same reason.
    pub fn preview_scope(&self, index: usize, points: usize) -> Vec<f32> {
        self.preview_scope_multi(index, points).into_iter().next().unwrap_or_default()
    }

    /// The same as [`preview_scope`](Self::preview_scope), but every
    /// channel — see [`track_scope_multi`](Self::track_scope_multi).
    pub fn preview_scope_multi(&self, index: usize, points: usize) -> Vec<Vec<f32>> {
        let Some(pos) = self.core.preview_pos.get(index) else {
            return Vec::new();
        };
        let position = pos.load(Ordering::Relaxed);
        if position == IDLE {
            return Vec::new();
        }
        self.core.preview_scope_multi(index, position, self.scope_half(), points)
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
        let tracks = tracks.into_iter().map(|t| (t, 1)).collect();
        Core::new(tracks, Vec::new(), 1)
    }

    /// Every track passed in is at `mix_channels` channels natively (no
    /// upmixing involved) — see [`core_with_native_channels`] for a mix
    /// where a narrower track's native count differs from the shared one.
    fn core_multi(tracks: Vec<Vec<f32>>, mix_channels: usize) -> Core {
        let tracks = tracks.into_iter().map(|t| (t, mix_channels)).collect();
        Core::new(tracks, Vec::new(), mix_channels)
    }

    /// Tracks paired with their own native channel count, which may be
    /// narrower than `mix_channels` (already upmixed to it, as
    /// `Engine::with_previews` would do).
    fn core_with_native_channels(tracks: Vec<(Vec<f32>, usize)>, mix_channels: usize) -> Core {
        Core::new(tracks, Vec::new(), mix_channels)
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
    fn scope_channels_with_one_channel_matches_scope_window() {
        let data = [0.0, 0.5, -0.9, 0.2, 0.0, 0.0];
        let mono = scope_channels(&data, 1, 2, 4, 4, 1.0);
        assert_eq!(mono.len(), 1);
        assert_eq!(mono[0], scope_window(&data, 2, 4, 4));
    }

    #[test]
    fn scope_channels_de_interleaves_before_windowing_and_scales_by_gain() {
        // Stereo: left is a rising ramp, right is silence throughout.
        let left = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
        let interleaved: Vec<f32> = left.iter().flat_map(|&l| [l, 0.0]).collect();

        let channels = scope_channels(&interleaved, 2, 3, 3, 3, 2.0);
        assert_eq!(channels.len(), 2);
        // Left, scaled by gain 2.0, matches windowing the left channel alone.
        let expected_left: Vec<f32> = scope_window(&left, 3, 3, 3).into_iter().map(|v| v * 2.0).collect();
        assert_eq!(channels[0], expected_left);
        // Right is silent throughout.
        assert!(channels[1].iter().all(|v| *v == 0.0));
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
        let c = Core::new(vec![(vec![0.0; 8], 1)], vec![(vec![0.1, 0.2], 1)], 1);
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
        let c = Core::new(vec![(vec![0.5; 2], 1)], vec![(vec![0.75; 2], 1)], 1);
        c.playing.store(true, Ordering::Relaxed);
        c.preview_pos[0].store(0, Ordering::Relaxed);
        assert_eq!(block(&c, 1, 1), vec![1.0]);
    }

    #[test]
    fn stereo_tracks_mix_independently_per_channel_on_a_stereo_device() {
        // Track A: L=0.2, R=0.1 every frame. Track B: L=0.1, R=0.2.
        let a: Vec<f32> = (0..4).flat_map(|_| [0.2, 0.1]).collect();
        let b: Vec<f32> = (0..4).flat_map(|_| [0.1, 0.2]).collect();
        let c = core_multi(vec![a, b], 2);
        c.playing.store(true, Ordering::Relaxed);
        // One stereo frame: L = 0.2+0.1 = 0.3, R = 0.1+0.2 = 0.3 (peak 0.3, no gain).
        assert_eq!(block(&c, 1, 2), vec![0.3, 0.3]);
    }

    #[test]
    fn a_stereo_mix_averages_down_to_a_mono_device() {
        // L=1.0, R=0.0 every frame: averaged, a mono device hears 0.5.
        let track: Vec<f32> = (0..2).flat_map(|_| [1.0, 0.0]).collect();
        let c = core_multi(vec![track], 2);
        c.playing.store(true, Ordering::Relaxed);
        assert_eq!(block(&c, 2, 1), vec![0.5, 0.5]);
    }

    #[test]
    fn a_stereo_mix_passes_through_to_a_wider_device_and_silences_the_rest() {
        let track: Vec<f32> = (0..2).flat_map(|_| [0.6, 0.3]).collect();
        let c = core_multi(vec![track], 2);
        c.playing.store(true, Ordering::Relaxed);
        // 4-channel device: L, R, then silence on the extra two channels.
        assert_eq!(block(&c, 1, 4), vec![0.6, 0.3, 0.0, 0.0]);
    }

    #[test]
    fn a_mono_mix_still_duplicates_to_every_device_channel_when_stereo_is_supported() {
        // Guards that Core's new per-channel path reproduces the original,
        // already-tested mono behavior exactly when mix_channels is 1
        // (same scenario as playing_sums_tracks_and_replicates_mono_to_every_channel,
        // via core_multi instead of the plain mono `core` helper).
        let c = core_multi(vec![vec![0.25; 8], vec![0.25; 8]], 1);
        c.playing.store(true, Ordering::Relaxed);
        assert_eq!(block(&c, 2, 2), vec![0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn a_mono_track_s_scope_stays_one_trace_even_upmixed_alongside_a_stereo_one() {
        // Track 0 is mono, upmixed to 2 channels (duplicated L=R) to sit in
        // the same mix as track 1, which is genuinely stereo. Regression
        // test for a bug where every track's scope reported mix_channels
        // traces, so a mono sample looked stereo just because something
        // else in the composition was.
        let mono: Vec<f32> = (0..8).flat_map(|i| [i as f32 * 0.1; 2]).collect(); // duplicated L=R
        let stereo: Vec<f32> = (0..4).flat_map(|i| [i as f32 * 0.1, -(i as f32) * 0.1]).collect();
        let c = core_with_native_channels(vec![(mono, 1), (stereo, 2)], 2);

        let mono_scope = c.track_scope_multi(0, 0, 4, 4);
        assert_eq!(mono_scope.len(), 1, "a mono track must show exactly one trace");

        let stereo_scope = c.track_scope_multi(1, 0, 4, 4);
        assert_eq!(stereo_scope.len(), 2, "a stereo track must show both channels");
        // The two channels are genuinely different (not a duplicated mono).
        assert_ne!(stereo_scope[0], stereo_scope[1]);

        // Out of range: empty, not a panic.
        assert!(c.track_scope_multi(9, 0, 4, 4).is_empty());
    }

    #[test]
    fn a_mono_preview_s_scope_stays_one_trace_even_upmixed_alongside_a_stereo_one() {
        let mono: Vec<f32> = (0..8).flat_map(|i| [i as f32 * 0.1; 2]).collect();
        let stereo: Vec<f32> = (0..4).flat_map(|i| [i as f32 * 0.1, -(i as f32) * 0.1]).collect();
        let c = Core::new(Vec::new(), vec![(mono, 1), (stereo, 2)], 2);

        assert_eq!(c.preview_scope_multi(0, 0, 4, 4).len(), 1);
        assert_eq!(c.preview_scope_multi(1, 0, 4, 4).len(), 2);
    }
}
