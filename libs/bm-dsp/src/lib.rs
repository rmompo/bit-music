//! Numeric audio primitives shared by every bit-music component.
//!
//! Pure functions over `f32` buffers: no file I/O, no audio device, no
//! knowledge of the `.bm1` format. Everything here is safe to call from a
//! real-time context except where noted (`resample`, `mix_into` allocate).
//!
//! The pitch-shift method is the classic sampler/tracker one: change the
//! buffer's **playback speed** (resampling), exactly what would happen if
//! you sped up or slowed down a tape. This changes pitch and duration
//! together; there is no time-stretching that preserves duration.

/// A block of decoded audio, normalized to `[-1.0, 1.0]`, interleaved when
/// [`channels`](Self::channels) is more than 1 (frame 0's channels, then
/// frame 1's, ...) — the same layout `hound` itself uses.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    channels: u16,
}

impl Default for AudioBuffer {
    /// Empty, mono: `channels` defaults to `1`, not `0` (an unplayable,
    /// nonsensical buffer), so a default value is still a valid buffer.
    fn default() -> Self {
        Self { data: Vec::new(), sample_rate: 0, channels: 1 }
    }
}

impl AudioBuffer {
    /// A mono buffer.
    pub fn new(data: Vec<f32>, sample_rate: u32) -> Self {
        Self { data, sample_rate, channels: 1 }
    }

    /// A buffer of `channels` interleaved channels (`channels: 1` is the
    /// same as [`new`](Self::new); `0` is treated as `1`, since a buffer
    /// always has at least one channel to be meaningful).
    pub fn new_multi(data: Vec<f32>, sample_rate: u32, channels: u16) -> Self {
        Self { data, sample_rate, channels: channels.max(1) }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Number of sample frames (`data.len() / channels`, so it is the same
    /// as `data.len()` for a mono buffer).
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels as usize
    }

    /// Duration in seconds (`0.0` if the sample rate is zero).
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.sample_rate as f64
        }
    }
}

/// Converts a distance in semitones to a resampling ratio
/// (`2^(semitones / 12)`): `0` -> `1.0`, `+12` -> `2.0`, `-12` -> `0.5`.
pub fn semitones_to_ratio(semitones: i32) -> f64 {
    2f64.powf(semitones as f64 / 12.0)
}

/// Averages interleaved channels down to mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resamples `input` according to `ratio` (linear interpolation):
/// - `ratio > 1.0` -> faster, higher-pitched playback, shorter buffer.
/// - `ratio < 1.0` -> slower, lower-pitched playback, longer buffer.
/// - `ratio == 1.0` -> unchanged.
///
/// The same math also converts between sample rates: to convert a buffer
/// from rate `A` to rate `B` without changing pitch or duration, use
/// `ratio = A / B`.
pub fn resample(input: &[f32], ratio: f64) -> Vec<f32> {
    if input.is_empty() || ratio <= 0.0 {
        return Vec::new();
    }

    let output_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut output = Vec::with_capacity(output_len);
    let last_index = input.len() - 1;

    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let idx = (src_pos.floor() as usize).min(last_index);
        let frac = (src_pos - idx as f64) as f32;
        let a = input[idx];
        let b = input[(idx + 1).min(last_index)];
        output.push(a + (b - a) * frac);
    }

    output
}

/// The same as [`resample`], on interleaved `input` of `channels` channels
/// (see [`AudioBuffer`]): each channel is resampled on its own — the
/// interpolation never blends samples from different channels together —
/// then the results are re-interleaved. `channels: 1` (or `0`, treated as
/// `1`) delegates straight to [`resample`], so it behaves identically for
/// mono input.
pub fn resample_multi(input: &[f32], ratio: f64, channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    if channels == 1 {
        return resample(input, ratio);
    }

    let per_channel: Vec<Vec<f32>> = (0..channels)
        .map(|c| input.iter().skip(c).step_by(channels).copied().collect())
        .collect();
    let resampled: Vec<Vec<f32>> = per_channel.iter().map(|ch| resample(ch, ratio)).collect();

    let frames = resampled.first().map_or(0, Vec::len);
    let mut output = Vec::with_capacity(frames * channels);
    for i in 0..frames {
        for ch in &resampled {
            output.push(ch[i]);
        }
    }
    output
}

/// Sums `voice` into `master` starting at frame `start`, extending
/// `master` with silence if needed.
pub fn mix_into(master: &mut Vec<f32>, voice: &[f32], start: usize) {
    let end = start + voice.len();
    if master.len() < end {
        master.resize(end, 0.0);
    }
    for (i, &value) in voice.iter().enumerate() {
        master[start + i] += value;
    }
}

/// The same as [`mix_into`], addressed by frame instead of by sample:
/// sums interleaved `voice` (`channels` channels) into interleaved
/// `master` starting at frame `start_frame`. `mix_into` is already
/// channel-layout agnostic (it only sums sample-for-sample at an offset),
/// so this is exactly `mix_into` with the offset converted from frames to
/// samples; `channels: 1` behaves identically to `mix_into`.
pub fn mix_into_multi(master: &mut Vec<f32>, voice: &[f32], start_frame: usize, channels: usize) {
    mix_into(master, voice, start_frame * channels.max(1));
}

/// Absolute peak of a buffer (`0.0` for an empty one).
pub fn peak(buffer: &[f32]) -> f32 {
    buffer.iter().fold(0.0f32, |acc, &v| acc.max(v.abs()))
}

/// Gain that brings `peak` down to `1.0` when it exceeds it, `1.0`
/// otherwise (never amplifies).
pub fn normalization_gain(peak: f32) -> f32 {
    if peak > 1.0 { 1.0 / peak } else { 1.0 }
}

/// Scales the buffer so its absolute peak doesn't exceed `1.0` (avoids
/// clipping when several voices overlap). No-op if it already fits.
pub fn normalize(buffer: &mut [f32]) {
    let gain = normalization_gain(peak(buffer));
    if gain != 1.0 {
        for v in buffer.iter_mut() {
            *v *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_with_ratio_one_keeps_input_unchanged() {
        let input = vec![0.0, 0.25, 0.5, 0.75, 1.0];
        let output = resample(&input, 1.0);
        assert_eq!(output.len(), input.len());
        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn resample_with_ratio_two_halves_the_length() {
        assert_eq!(resample(&vec![0.0; 100], 2.0).len(), 50);
    }

    #[test]
    fn resample_with_half_ratio_doubles_the_length() {
        assert_eq!(resample(&vec![0.0; 100], 0.5).len(), 200);
    }

    #[test]
    fn downmix_averages_stereo_channels() {
        // L=1.0, R=0.0 -> mono = 0.5, over two frames.
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn new_multi_with_one_channel_matches_new() {
        let mono = AudioBuffer::new(vec![0.1, 0.2, 0.3], 44100);
        let same = AudioBuffer::new_multi(vec![0.1, 0.2, 0.3], 44100, 1);
        assert_eq!(mono.channels(), same.channels());
        assert_eq!(mono.frames(), same.frames());
        assert_eq!(mono.duration_seconds(), same.duration_seconds());
        // 0 channels makes no sense for a buffer: treated as 1.
        let zero = AudioBuffer::new_multi(vec![0.1, 0.2, 0.3], 44100, 0);
        assert_eq!(zero.channels(), 1);
    }

    #[test]
    fn frames_and_duration_account_for_the_channel_count() {
        // 3 frames of stereo (6 samples) at 44100 Hz.
        let stereo = AudioBuffer::new_multi(vec![0.0; 6], 44100, 2);
        assert_eq!(stereo.frames(), 3);
        assert!((stereo.duration_seconds() - 3.0 / 44100.0).abs() < 1e-12);
    }

    #[test]
    fn default_audio_buffer_is_empty_mono_not_zero_channels() {
        let default = AudioBuffer::default();
        assert_eq!(default.channels(), 1);
        assert!(default.data.is_empty());
    }

    #[test]
    fn resample_multi_with_one_channel_matches_resample() {
        let input = vec![0.0, 0.25, 0.5, 0.75, 1.0, 0.9, 0.8];
        assert_eq!(resample_multi(&input, 1.7, 1), resample(&input, 1.7));
        // channels: 0 is treated as 1, same as AudioBuffer::new_multi.
        assert_eq!(resample_multi(&input, 1.7, 0), resample(&input, 1.7));
    }

    #[test]
    fn resample_multi_never_blends_channels_together() {
        // Left is a rising ramp, right is a falling one: if resample_multi
        // blended samples across the interleave boundary, both channels
        // would end up with values from the other.
        let left: Vec<f32> = (0..20).map(|i| i as f32 / 19.0).collect();
        let right: Vec<f32> = left.iter().map(|v| 1.0 - v).collect();
        let interleaved: Vec<f32> = left.iter().zip(&right).flat_map(|(&l, &r)| [l, r]).collect();

        let out = resample_multi(&interleaved, 1.3, 2);
        let out_left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let out_right: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();

        // Each channel resampled on its own matches resampling it directly.
        assert_eq!(out_left, resample(&left, 1.3));
        assert_eq!(out_right, resample(&right, 1.3));
        // And the two channels stay distinct (not blended into each other).
        assert_ne!(out_left, out_right);
    }

    #[test]
    fn mix_into_multi_with_one_channel_matches_mix_into() {
        let mut a = vec![0.1, 0.1, 0.1, 0.1];
        let mut b = vec![0.1, 0.1, 0.1, 0.1];
        mix_into(&mut a, &[0.5, 0.5], 1);
        mix_into_multi(&mut b, &[0.5, 0.5], 1, 1);
        assert_eq!(a, b);
    }

    #[test]
    fn mix_into_multi_is_addressed_by_frame_not_by_sample() {
        // Stereo: frame 1 starts at sample offset 2 (frame * channels).
        let mut master = vec![0.0; 6]; // 3 stereo frames
        mix_into_multi(&mut master, &[0.5, 0.25], 1, 2);
        assert_eq!(master, vec![0.0, 0.0, 0.5, 0.25, 0.0, 0.0]);
    }

    #[test]
    fn semitones_to_ratio_matches_octaves() {
        assert!((semitones_to_ratio(0) - 1.0).abs() < 1e-9);
        assert!((semitones_to_ratio(12) - 2.0).abs() < 1e-9);
        assert!((semitones_to_ratio(-12) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn mix_into_sums_overlapping_voices() {
        let mut master = vec![0.1, 0.1, 0.1, 0.1];
        mix_into(&mut master, &[0.5, 0.5], 1);
        assert_eq!(master, vec![0.1, 0.6, 0.6, 0.1]);
    }

    #[test]
    fn mix_into_extends_buffer_when_voice_goes_past_the_end() {
        let mut master = vec![0.0, 0.0];
        mix_into(&mut master, &[1.0, 1.0], 1);
        assert_eq!(master, vec![0.0, 1.0, 1.0]);
    }

    #[test]
    fn normalize_scales_down_when_peak_exceeds_one() {
        let mut buffer = vec![0.5, -2.0, 1.0];
        normalize(&mut buffer);
        assert!((buffer[1] + 1.0).abs() < 1e-6); // -2.0 / 2.0 == -1.0
        assert!((buffer[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn normalize_leaves_buffer_untouched_when_within_range() {
        let mut buffer = vec![0.5, -0.5, 0.25];
        let original = buffer.clone();
        normalize(&mut buffer);
        assert_eq!(buffer, original);
    }

    #[test]
    fn audio_buffer_duration_in_seconds() {
        assert!((AudioBuffer::new(vec![0.0; 22050], 44100).duration_seconds() - 0.5).abs() < 1e-9);
        assert_eq!(AudioBuffer::new(vec![0.0; 10], 0).duration_seconds(), 0.0);
    }
}
