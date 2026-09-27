//! Reading, writing and header-checking `.wav` files.
//!
//! Decoded audio is returned as a mono [`bm_dsp::AudioBuffer`]: multi-channel
//! files are downmixed on load because the bit-music mixing engine works in
//! mono. If true stereo is wanted in the future, this is the point to extend.

use std::path::Path;

use bm_dsp::AudioBuffer;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WavError {
    #[error("could not read wav '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: hound::Error,
    },
    #[error("could not write wav '{path}': {source}")]
    Write {
        path: String,
        #[source]
        source: hound::Error,
    },
}

/// Decodes a `.wav` file (integer or float PCM, any bit depth) to a mono
/// buffer normalized to `[-1.0, 1.0]`.
pub fn load_wav(path: &Path) -> Result<AudioBuffer, WavError> {
    let reader = hound::WavReader::open(path).map_err(|source| WavError::Read {
        path: path.display().to_string(),
        source,
    })?;
    decode(reader)
}

/// The same as [`load_wav`], but from bytes already in memory (e.g. a
/// sample read out of a `.bmz` package) instead of a file on disk. `label`
/// is only used to name the file in a resulting error.
pub fn load_wav_bytes(bytes: &[u8], label: &str) -> Result<AudioBuffer, WavError> {
    let reader =
        hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|source| WavError::Read {
            path: label.to_string(),
            source,
        })?;
    decode(reader)
}

/// Shared decode step for [`load_wav`] and [`load_wav_bytes`].
fn decode<R: std::io::Read>(mut reader: hound::WavReader<R>) -> Result<AudioBuffer, WavError> {
    let spec = reader.spec();
    let channels = spec.channels as usize;

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / max)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
    };

    Ok(AudioBuffer::new(
        bm_dsp::downmix(&interleaved, channels),
        spec.sample_rate,
    ))
}

/// Cheaply checks that `path` exists and is a well-formed `.wav` file,
/// reading only its header (samples are not decoded).
pub fn check_wav(path: &Path) -> Result<(), WavError> {
    hound::WavReader::open(path)
        .map(|_| ())
        .map_err(|source| WavError::Read {
            path: path.display().to_string(),
            source,
        })
}

/// The same as [`check_wav`], but on bytes already in memory. `label` is
/// only used to name the file in a resulting error.
pub fn check_wav_bytes(bytes: &[u8], label: &str) -> Result<(), WavError> {
    hound::WavReader::new(std::io::Cursor::new(bytes))
        .map(|_| ())
        .map_err(|source| WavError::Read {
            path: label.to_string(),
            source,
        })
}

/// A [`WavError`] for a sample that a [`SampleSource`](../bm_project/enum.SampleSource.html)
/// could not find (e.g. missing from a `.bmz` package).
pub fn missing(label: &str) -> WavError {
    WavError::Read {
        path: label.to_string(),
        source: hound::Error::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found in the package",
        )),
    }
}

/// Writes `data` (mono, at `sample_rate`) as a 16-bit PCM `.wav` file,
/// clamping to `[-1.0, 1.0]` before converting to integer samples.
pub fn write_wav(path: &Path, data: &[f32], sample_rate: u32) -> Result<(), WavError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let to_err = |source: hound::Error| WavError::Write {
        path: path.display().to_string(),
        source,
    };

    let mut writer = hound::WavWriter::create(path, spec).map_err(to_err)?;
    for &sample in data {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        writer.write_sample(value).map_err(to_err)?;
    }
    writer.finalize().map_err(to_err)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wav(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bm-wav-{name}-{}.wav", std::process::id()))
    }

    #[test]
    fn write_then_load_round_trips_within_16bit_precision() {
        let path = temp_wav("roundtrip");
        let original = vec![-1.0, -0.5, 0.0, 0.5, 1.0];
        write_wav(&path, &original, 44100).expect("write_wav should succeed");

        let loaded = load_wav(&path).expect("load_wav should succeed");
        check_wav(&path).expect("check_wav should accept a valid file");
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.data.len(), original.len());
        for (a, b) in original.iter().zip(loaded.data.iter()) {
            assert!((a - b).abs() < 1e-3, "expected {a}, got {b}");
        }
    }

    #[test]
    fn bytes_variants_match_the_path_ones() {
        let path = temp_wav("bytes-roundtrip");
        let original = vec![-1.0, -0.5, 0.0, 0.5, 1.0];
        write_wav(&path, &original, 44100).expect("write_wav should succeed");
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();

        check_wav_bytes(&bytes, "kick.wav").expect("check_wav_bytes should accept a valid file");
        let loaded = load_wav_bytes(&bytes, "kick.wav").expect("load_wav_bytes should succeed");
        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.data.len(), original.len());

        assert!(check_wav_bytes(b"not a wav file", "bad.wav").is_err());
        assert!(load_wav_bytes(b"not a wav file", "bad.wav").is_err());
    }

    #[test]
    fn missing_reports_the_label_and_is_not_found() {
        let err = missing("samples/kick.wav");
        assert!(err.to_string().contains("samples/kick.wav"));
    }

    #[test]
    fn check_wav_rejects_missing_and_malformed_files() {
        assert!(check_wav(&temp_wav("does-not-exist")).is_err());

        let bad = temp_wav("malformed");
        std::fs::write(&bad, b"this is not a wav file").unwrap();
        let result = check_wav(&bad);
        std::fs::remove_file(&bad).ok();
        assert!(result.is_err());
    }
}
