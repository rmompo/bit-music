//! Reading, writing and header-checking `.wav` files.
//!
//! `load_wav`/`load_wav_bytes`/`write_wav` decode/encode to and from a
//! mono [`bm_dsp::AudioBuffer`] (multi-channel files are downmixed on
//! load) — the shape the mixing/playback pipeline uses today. The
//! `_multi` siblings (`load_wav_multi`, `load_wav_bytes_multi`,
//! `write_wav_multi`) keep every channel instead, for the parts of the
//! pipeline that have moved to real multi-channel support (see
//! `specs/stereo-audio.md`).

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
    let reader = open_path(path)?;
    let (interleaved, sample_rate, channels) = decode_raw(reader);
    Ok(AudioBuffer::new(bm_dsp::downmix(&interleaved, channels as usize), sample_rate))
}

/// The same as [`load_wav`], but from bytes already in memory (e.g. a
/// sample read out of a `.bmz` package) instead of a file on disk. `label`
/// is only used to name the file in a resulting error.
pub fn load_wav_bytes(bytes: &[u8], label: &str) -> Result<AudioBuffer, WavError> {
    let reader = open_bytes(bytes, label)?;
    let (interleaved, sample_rate, channels) = decode_raw(reader);
    Ok(AudioBuffer::new(bm_dsp::downmix(&interleaved, channels as usize), sample_rate))
}

/// The same as [`load_wav`], but keeps every channel instead of downmixing
/// to mono (a mono file still comes back as a 1-channel buffer).
pub fn load_wav_multi(path: &Path) -> Result<AudioBuffer, WavError> {
    let reader = open_path(path)?;
    let (interleaved, sample_rate, channels) = decode_raw(reader);
    Ok(AudioBuffer::new_multi(interleaved, sample_rate, channels))
}

/// The same as [`load_wav_multi`], from bytes already in memory (see
/// [`load_wav_bytes`]).
pub fn load_wav_bytes_multi(bytes: &[u8], label: &str) -> Result<AudioBuffer, WavError> {
    let reader = open_bytes(bytes, label)?;
    let (interleaved, sample_rate, channels) = decode_raw(reader);
    Ok(AudioBuffer::new_multi(interleaved, sample_rate, channels))
}

fn open_path(path: &Path) -> Result<hound::WavReader<std::io::BufReader<std::fs::File>>, WavError> {
    hound::WavReader::open(path).map_err(|source| WavError::Read {
        path: path.display().to_string(),
        source,
    })
}

fn open_bytes<'a>(bytes: &'a [u8], label: &str) -> Result<hound::WavReader<std::io::Cursor<&'a [u8]>>, WavError> {
    hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|source| WavError::Read {
        path: label.to_string(),
        source,
    })
}

/// Decodes every sample into an interleaved buffer, without downmixing:
/// the raw step shared by every `load_wav*` variant. Returns the samples,
/// the file's sample rate, and its channel count.
fn decode_raw<R: std::io::Read>(mut reader: hound::WavReader<R>) -> (Vec<f32>, u32, u16) {
    let spec = reader.spec();

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

    (interleaved, spec.sample_rate, spec.channels)
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
    write_wav_multi(path, data, sample_rate, 1)
}

/// The same as [`write_wav`], for `channels` interleaved channels of
/// `data` (`channels: 1` is exactly [`write_wav`]).
pub fn write_wav_multi(path: &Path, data: &[f32], sample_rate: u32, channels: u16) -> Result<(), WavError> {
    let spec = hound::WavSpec {
        channels: channels.max(1),
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
    fn write_wav_delegates_to_write_wav_multi_with_one_channel() {
        let path = temp_wav("mono-delegates");
        let data = vec![-1.0, -0.5, 0.0, 0.5, 1.0];
        write_wav(&path, &data, 44100).unwrap();
        let loaded = load_wav_multi(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded.channels(), 1);
        assert_eq!(loaded.frames(), data.len());
    }

    #[test]
    fn multi_round_trips_a_stereo_file_without_downmixing() {
        let path = temp_wav("stereo-roundtrip");
        // Interleaved stereo: left is a rising ramp, right is silence.
        let original = vec![-1.0, 0.0, -0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0];
        write_wav_multi(&path, &original, 44100, 2).expect("write_wav_multi should succeed");

        let loaded = load_wav_multi(&path).expect("load_wav_multi should succeed");
        check_wav(&path).expect("check_wav should still accept a stereo file");
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.channels(), 2);
        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.frames(), 5);
        assert_eq!(loaded.data.len(), original.len());
        for (a, b) in original.iter().zip(loaded.data.iter()) {
            assert!((a - b).abs() < 1e-3, "expected {a}, got {b}");
        }
        // The right channel (every odd sample) really is silent, not a
        // downmix blend of L and R.
        assert!(loaded.data.iter().skip(1).step_by(2).all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn load_wav_still_downmixes_the_same_stereo_file_multi_does_not() {
        let path = temp_wav("downmix-vs-multi");
        // L=1.0, R=0.0 on every frame.
        let original = vec![1.0, 0.0, 1.0, 0.0];
        write_wav_multi(&path, &original, 44100, 2).unwrap();

        let mono = load_wav(&path).unwrap();
        let stereo = load_wav_multi(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(mono.channels(), 1);
        assert_eq!(mono.frames(), 2);
        assert!((mono.data[0] - 0.5).abs() < 1e-3); // downmixed average

        assert_eq!(stereo.channels(), 2);
        assert_eq!(stereo.frames(), 2);
        assert!((stereo.data[0] - 1.0).abs() < 1e-3); // left kept as-is
        assert!(stereo.data[1].abs() < 1e-3); // right kept as-is
    }

    #[test]
    fn bytes_multi_matches_the_path_variant() {
        let path = temp_wav("stereo-bytes");
        let original = vec![0.25, -0.25, 0.5, -0.5];
        write_wav_multi(&path, &original, 44100, 2).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();

        let loaded = load_wav_bytes_multi(&bytes, "stereo.wav").expect("load_wav_bytes_multi should succeed");
        assert_eq!(loaded.channels(), 2);
        assert_eq!(loaded.data.len(), original.len());
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
