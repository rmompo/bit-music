//! Semantic validation of the deserialized model (business rules `serde`
//! cannot enforce on its own) and resolution of the default
//! `sample.rootNote`/`rootOctave` values from `metadata.others`.

use std::collections::HashSet;

use thiserror::Error;

use crate::model::{Composition, Metadata};
use crate::note::{self, NoteParseError};

const DEFAULT_ROOT_NOTE_KEY: &str = "sampleDefaultNote";
const DEFAULT_ROOT_OCTAVE_KEY: &str = "sampleDefaultOctave";
const FALLBACK_ROOT_NOTE: &str = "C";
const FALLBACK_ROOT_OCTAVE: u8 = 4;

/// `.bm1` **format** versions this build of `bm` understands —
/// distinct from `bm`'s own tool version (`env!("CARGO_PKG_VERSION")`,
/// printed by `bm version`). `metadata.version` in a loaded composition
/// must be one of these, or it is rejected: this is what makes `version` a
/// real compatibility guard instead of decorative metadata.
pub const SUPPORTED_FORMAT_VERSIONS: [&str; 1] = ["1.0"];

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("pattern '{id}': steps length must be a multiple of 4 (got {len})")]
    StepsNotMultipleOfFour { id: String, len: usize },

    #[error("pattern '{id}': invalid note '{note}' at steps[{index}]: {source}")]
    InvalidNote {
        id: String,
        index: usize,
        note: String,
        #[source]
        source: NoteParseError,
    },

    #[error("track '{track_id}': reference to unknown pattern '{pattern_id}'")]
    UnknownPatternReference { track_id: String, pattern_id: String },

    #[error("pattern '{pattern_id}': reference to unknown sample '{sample_id}'")]
    UnknownSampleReference {
        pattern_id: String,
        sample_id: String,
    },

    #[error("sample '{id}': invalid rootNote '{note}'")]
    InvalidRootNote { id: String, note: String },

    #[error("sample '{id}': rootOctave out of range (0-8), got {octave}")]
    RootOctaveOutOfRange { id: String, octave: u8 },

    #[error("metadata.others['{DEFAULT_ROOT_NOTE_KEY}'] is invalid: '{0}'")]
    InvalidDefaultRootNote(String),

    #[error("metadata.others['{DEFAULT_ROOT_OCTAVE_KEY}'] is invalid: '{0}' (must be an integer 0-8)")]
    InvalidDefaultRootOctave(String),

    #[error(
        "unsupported metadata.version '{found}' (this build of bm supports: {})",
        SUPPORTED_FORMAT_VERSIONS.join(", ")
    )]
    UnsupportedFormatVersion { found: String },

    #[error("metadata.bpm must be greater than 0")]
    InvalidBpm,

    #[error("metadata.stepsPerBeat must be greater than 0")]
    InvalidStepsPerBeat,

    #[error("two samples share the same id '{id}'")]
    DuplicateSampleId { id: String },

    #[error("two patterns share the same id '{id}'")]
    DuplicatePatternId { id: String },

    #[error("two tracks share the same id '{id}'")]
    DuplicateTrackId { id: String },

    #[error("the composition has no samples")]
    NoSamples,

    #[error("the composition has no patterns")]
    NoPatterns,

    #[error("the arrangement has no tracks")]
    NoTracks,
}

/// Resolves the default `rootNote`/`rootOctave` for samples that don't
/// specify one: first looks in `metadata.others` (`sampleDefaultNote` /
/// `sampleDefaultOctave`), and falls back to "C"/4 if those aren't there.
pub fn resolve_sample_defaults(metadata: &Metadata) -> Result<(String, u8), ValidationError> {
    let default_note = match find_other(metadata, DEFAULT_ROOT_NOTE_KEY) {
        Some(value) => {
            if !note::is_valid_note_name(value) {
                return Err(ValidationError::InvalidDefaultRootNote(value.to_string()));
            }
            value.to_string()
        }
        None => FALLBACK_ROOT_NOTE.to_string(),
    };

    let default_octave = match find_other(metadata, DEFAULT_ROOT_OCTAVE_KEY) {
        Some(value) => value
            .parse::<u8>()
            .ok()
            .filter(|o| *o <= 8)
            .ok_or_else(|| ValidationError::InvalidDefaultRootOctave(value.to_string()))?,
        None => FALLBACK_ROOT_OCTAVE,
    };

    Ok((default_note, default_octave))
}

/// Fills `root_note`/`root_octave` on every sample that omits them, using
/// the defaults from `resolve_sample_defaults`. After this call every
/// sample has both fields set.
pub fn apply_sample_defaults(composition: &mut Composition) -> Result<(), ValidationError> {
    let (default_note, default_octave) = resolve_sample_defaults(&composition.metadata)?;
    for sample in &mut composition.samples {
        if sample.root_note.is_none() {
            sample.root_note = Some(default_note.clone());
        }
        if sample.root_octave.is_none() {
            sample.root_octave = Some(default_octave);
        }
    }
    Ok(())
}

fn find_other<'a>(metadata: &'a Metadata, key: &str) -> Option<&'a str> {
    metadata
        .others
        .iter()
        .find(|kv| kv.key == key)
        .map(|kv| kv.value.as_str())
}

pub fn validate(composition: &Composition) -> Result<(), ValidationError> {
    if !SUPPORTED_FORMAT_VERSIONS.contains(&composition.metadata.version.as_str()) {
        return Err(ValidationError::UnsupportedFormatVersion {
            found: composition.metadata.version.clone(),
        });
    }
    if composition.metadata.bpm == 0 {
        return Err(ValidationError::InvalidBpm);
    }
    if composition.metadata.steps_per_beat == 0 {
        return Err(ValidationError::InvalidStepsPerBeat);
    }
    if composition.samples.is_empty() {
        return Err(ValidationError::NoSamples);
    }
    if composition.patterns.is_empty() {
        return Err(ValidationError::NoPatterns);
    }
    if composition.arrangement.tracks.is_empty() {
        return Err(ValidationError::NoTracks);
    }

    let mut sample_ids: HashSet<&str> = HashSet::new();
    for sample in &composition.samples {
        if !sample_ids.insert(sample.id.as_str()) {
            return Err(ValidationError::DuplicateSampleId {
                id: sample.id.clone(),
            });
        }

        if let Some(root_note) = &sample.root_note {
            if !note::is_valid_note_name(root_note) {
                return Err(ValidationError::InvalidRootNote {
                    id: sample.id.clone(),
                    note: root_note.clone(),
                });
            }
        }

        if let Some(root_octave) = sample.root_octave {
            if root_octave > 8 {
                return Err(ValidationError::RootOctaveOutOfRange {
                    id: sample.id.clone(),
                    octave: root_octave,
                });
            }
        }
    }

    let mut pattern_ids: HashSet<&str> = HashSet::new();
    for pattern in &composition.patterns {
        if !pattern_ids.insert(pattern.id.as_str()) {
            return Err(ValidationError::DuplicatePatternId {
                id: pattern.id.clone(),
            });
        }

        if !sample_ids.contains(pattern.sample.as_str()) {
            return Err(ValidationError::UnknownSampleReference {
                pattern_id: pattern.id.clone(),
                sample_id: pattern.sample.clone(),
            });
        }

        if pattern.steps.len() % 4 != 0 {
            return Err(ValidationError::StepsNotMultipleOfFour {
                id: pattern.id.clone(),
                len: pattern.steps.len(),
            });
        }

        for (index, step) in pattern.steps.iter().enumerate() {
            if let Some(raw_note) = step {
                if let Err(source) = note::parse_note(raw_note) {
                    return Err(ValidationError::InvalidNote {
                        id: pattern.id.clone(),
                        index,
                        note: raw_note.clone(),
                        source,
                    });
                }
            }
        }
    }

    let mut track_ids: HashSet<&str> = HashSet::new();
    for track in &composition.arrangement.tracks {
        if !track_ids.insert(track.id.as_str()) {
            return Err(ValidationError::DuplicateTrackId {
                id: track.id.clone(),
            });
        }

        for slot in &track.sequence {
            if let Some(pattern_id) = slot {
                if !pattern_ids.contains(pattern_id.as_str()) {
                    return Err(ValidationError::UnknownPatternReference {
                        track_id: track.id.clone(),
                        pattern_id: pattern_id.clone(),
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Arrangement, KeyValue, Pattern, Sample, Track};

    fn valid_composition(version: &str) -> Composition {
        Composition {
            metadata: Metadata {
                version: version.to_string(),
                title: "test".to_string(),
                bpm: 120,
                steps_per_beat: 4,
                others: Vec::<KeyValue>::new(),
            },
            samples: vec![Sample {
                id: "s".to_string(),
                file: "s.wav".to_string(),
                root_note: Some("C".to_string()),
                root_octave: Some(4),
            }],
            patterns: vec![Pattern {
                id: "p".to_string(),
                sample: "s".to_string(),
                steps: vec![Some("C4".to_string()), None, None, None],
            }],
            arrangement: Arrangement {
                tracks: vec![Track {
                    id: "t".to_string(),
                    sequence: vec![Some("p".to_string())],
                }],
            },
        }
    }

    #[test]
    fn accepts_a_supported_format_version() {
        assert!(validate(&valid_composition("1.0")).is_ok());
    }

    #[test]
    fn rejects_an_unsupported_format_version() {
        let err = validate(&valid_composition("2.0")).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::UnsupportedFormatVersion { found } if found == "2.0"
        ));
    }
}
