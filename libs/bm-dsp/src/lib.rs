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

/// A block of decoded mono audio, normalized to `[-1.0, 1.0]`.
#[derive(Debug, Clone, Default)]
pub struct AudioBuffer {
    pub data: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioBuffer {
    pub fn new(data: Vec<f32>, sample_rate: u32) -> Self {
        Self { data, sample_rate }
    }

    /// Duration in seconds (`0.0` if the sample rate is zero).
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.data.len() as f64 / self.sample_rate as f64
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
