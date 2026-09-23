//! Generates the demo samples used by `demos` from plain code, so the
//! project's audio material is entirely its own (no third-party recordings)
//! and anyone can regenerate or inspect it.
//!
//! Usage (from the repository root):
//!
//! ```text
//! cargo run -p gen-demo-samples                 # writes demos/samples
//! cargo run -p gen-demo-samples -- some/dir     # writes elsewhere
//! ```
//!
//! Every sample is mono, 44.1 kHz, and fully deterministic (the noise comes
//! from a fixed-seed generator), so regenerating gives identical files.
//!
//! Pitched samples are synthesized at the note their `.bm1` declares as
//! `rootNote`/`rootOctave`, because the player pitch-shifts relative to it:
//! the e-piano and the sax are both **C3**. The percussion is unpitched and
//! uses the default root (C4), so it plays unshifted at C4.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SAMPLE_RATE: u32 = 44_100;
/// C3 in equal temperament with A4 = 440 Hz.
const C3_HZ: f32 = 130.812_78;

/// Deterministic white noise in `[-1, 1)` (xorshift32).
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn frames(seconds: f32) -> usize {
    (seconds * SAMPLE_RATE as f32).round() as usize
}

fn time(i: usize) -> f32 {
    i as f32 / SAMPLE_RATE as f32
}

/// Applies a linear fade-out over the last `fade_seconds` (so no sample
/// ends with a click) and scales the buffer to the given absolute peak.
fn finish(mut data: Vec<f32>, peak: f32, fade_seconds: f32) -> Vec<f32> {
    let fade = frames(fade_seconds).min(data.len());
    let len = data.len();
    for (k, v) in data[len - fade..].iter_mut().enumerate() {
        *v *= 1.0 - (k as f32 + 1.0) / fade as f32;
    }
    let current = data.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if current > 0.0 {
        let gain = peak / current;
        data.iter_mut().for_each(|v| *v *= gain);
    }
    data
}

/// Bass drum: a sine sweeping down from 150 Hz to 45 Hz with a short click.
fn kick() -> Vec<f32> {
    let mut noise = Noise::new(0x4B49_434B);
    let mut phase = 0.0f32;
    let data = (0..frames(0.5))
        .map(|i| {
            let t = time(i);
            let freq = 45.0 + 105.0 * (-t / 0.035).exp();
            phase += TAU * freq / SAMPLE_RATE as f32;
            let body = phase.sin() * (-t / 0.16).exp();
            let click = noise.next() * (-t / 0.001).exp() * 0.3;
            body + click
        })
        .collect();
    finish(data, 0.9, 0.02)
}

/// Snare: a noise burst (crudely high-passed) over two short tones.
fn snare() -> Vec<f32> {
    let mut noise = Noise::new(0x534E_4152);
    let mut previous = 0.0f32;
    let data = (0..frames(0.25))
        .map(|i| {
            let t = time(i);
            let raw = noise.next();
            let high = raw - 0.6 * previous;
            previous = raw;
            let snap = high * (-t / 0.07).exp() * 0.8;
            let tone = (TAU * 185.0 * t).sin() * (-t / 0.05).exp() * 0.5
                + (TAU * 330.0 * t).sin() * (-t / 0.035).exp() * 0.25;
            snap + tone
        })
        .collect();
    finish(data, 0.8, 0.02)
}

/// Closed hi-hat: very short, strongly high-passed noise plus a few
/// inharmonic partials for the metallic edge.
fn hihat() -> Vec<f32> {
    let mut noise = Noise::new(0x4849_4841);
    let (mut prev1, mut prev2) = (0.0f32, 0.0f32);
    let data = (0..frames(0.12))
        .map(|i| {
            let t = time(i);
            let raw = noise.next();
            let high = raw - 2.0 * prev1 + prev2; // second difference: sharp high-pass
            prev2 = prev1;
            prev1 = raw;
            let metal = (TAU * 6_200.0 * t).sin()
                + (TAU * 8_400.0 * t).sin()
                + (TAU * 9_700.0 * t).sin();
            (high * 0.25 + metal * 0.12) * (-t / 0.022).exp()
        })
        .collect();
    finish(data, 0.6, 0.015)
}

/// Electric piano at C3: two-operator FM whose modulation index decays (a
/// bright attack that mellows) plus a short bell-like tine, over a
/// percussive amplitude envelope.
fn epiano() -> Vec<f32> {
    let f = C3_HZ;
    let data = (0..frames(2.5))
        .map(|i| {
            let t = time(i);
            let index = 2.2 * (-t / 0.35).exp() + 0.25;
            let modulator = (TAU * f * t).sin();
            let carrier = (TAU * f * t + index * modulator).sin();
            let tine = (TAU * f * 14.0 * t).sin() * (-t / 0.03).exp() * 0.12;
            let envelope = (1.0 - (-t / 0.003).exp()) * (-t / 0.9).exp();
            (carrier + tine) * envelope
        })
        .collect();
    finish(data, 0.8, 0.1)
}

/// Saxophone-like lead at C3: the first twelve harmonics shaped by two
/// broad "reed" resonances, gentle delayed vibrato, a little breath noise
/// and a soft attack/release.
fn sax() -> Vec<f32> {
    let mut noise = Noise::new(0x5341_5800);
    let mut breath = 0.0f32;
    let mut phase = 0.0f32;
    let total = 1.2f32;
    let data = (0..frames(total))
        .map(|i| {
            let t = time(i);
            let vibrato_depth = ((t - 0.15) / 0.3).clamp(0.0, 1.0);
            let vibrato = 1.0 + 0.004 * vibrato_depth * (TAU * 5.5 * t).sin();
            phase += TAU * C3_HZ * vibrato / SAMPLE_RATE as f32;

            let mut tone = 0.0f32;
            for k in 1..=12u32 {
                let freq = C3_HZ * k as f32;
                let resonance = 1.0
                    + 1.8 * (-((freq - 900.0) / 500.0).powi(2)).exp()
                    + 0.8 * (-((freq - 1_800.0) / 700.0).powi(2)).exp();
                tone += (k as f32 * phase).sin() * resonance / (k as f32).powf(0.9);
            }

            breath += 0.15 * (noise.next() - breath); // low-passed noise
            let attack = (t / 0.05).clamp(0.0, 1.0);
            let release = ((total - t) / 0.2).clamp(0.0, 1.0);
            let envelope = attack * release * (1.0 + 0.03 * (TAU * 4.7 * t).sin());
            (tone * 0.5 + breath * 0.25) * envelope
        })
        .collect();
    finish(data, 0.8, 0.02)
}

/// Every demo sample, as `(file name, audio)`.
fn all_samples() -> Vec<(&'static str, Vec<f32>)> {
    vec![
        ("kick.wav", kick()),
        ("snare.wav", snare()),
        ("hihat.wav", hihat()),
        ("epiano.wav", epiano()),
        ("sax.wav", sax()),
    ]
}

fn main() -> ExitCode {
    let out_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("demos/samples"));

    if let Err(err) = std::fs::create_dir_all(&out_dir) {
        eprintln!("could not create '{}': {err}", out_dir.display());
        return ExitCode::FAILURE;
    }

    for (name, data) in all_samples() {
        let path: PathBuf = Path::new(&out_dir).join(name);
        match bm_wav::write_wav(&path, &data, SAMPLE_RATE) {
            Ok(()) => println!(
                "wrote {} ({} frames, {:.2} s)",
                path.display(),
                data.len(),
                data.len() as f32 / SAMPLE_RATE as f32
            ),
            Err(err) => {
                eprintln!("{err}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(data: &[f32]) -> f32 {
        data.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// Lag (in frames) with the strongest normalized autocorrelation inside
    /// the search range, measured over `window` starting at `start`.
    fn dominant_period(data: &[f32], start: usize, window: usize, lags: std::ops::Range<usize>) -> usize {
        let segment = &data[start..start + window + lags.end];
        lags.max_by(|&a, &b| {
            let score = |lag: usize| -> f32 {
                let (mut dot, mut e1, mut e2) = (0.0f32, 0.0f32, 0.0f32);
                for i in 0..window {
                    dot += segment[i] * segment[i + lag];
                    e1 += segment[i] * segment[i];
                    e2 += segment[i + lag] * segment[i + lag];
                }
                dot / (e1 * e2).sqrt().max(1e-9)
            };
            score(a).total_cmp(&score(b))
        })
        .unwrap()
    }

    #[test]
    fn durations_and_peaks_are_as_designed() {
        let expected = [
            ("kick.wav", 0.5, 0.9),
            ("snare.wav", 0.25, 0.8),
            ("hihat.wav", 0.12, 0.6),
            ("epiano.wav", 2.5, 0.8),
            ("sax.wav", 1.2, 0.8),
        ];
        let all = all_samples();
        assert_eq!(all.len(), expected.len());
        for ((name, data), (exp_name, seconds, exp_peak)) in all.iter().zip(expected) {
            assert_eq!(*name, exp_name);
            assert_eq!(data.len(), frames(seconds), "{name} length");
            assert!((peak(data) - exp_peak).abs() < 1e-4, "{name} peak {}", peak(data));
        }
    }

    #[test]
    fn every_sample_ends_silent_so_nothing_clicks() {
        for (name, data) in all_samples() {
            let tail = data.last().copied().unwrap().abs();
            assert!(tail < 0.01, "{name} ends at {tail}");
        }
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(all_samples(), all_samples());
    }

    #[test]
    fn pitched_samples_are_really_at_their_declared_root_c3() {
        // The player pitch-shifts relative to rootNote, so a sample that is
        // not actually at its declared note would play out of tune.
        let expected_period = SAMPLE_RATE as f32 / C3_HZ; // ~337 frames
        for (name, data) in [("epiano", epiano()), ("sax", sax())] {
            let lag = dominant_period(&data, frames(0.3), 2_000, 200..500);
            let error = (lag as f32 - expected_period).abs() / expected_period;
            assert!(error < 0.03, "{name}: period {lag} frames, expected ~{expected_period:.0}");
        }
    }
}
