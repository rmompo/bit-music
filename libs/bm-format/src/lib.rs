//! The bit-music `.bm1` format contract (see `specs/format.md`).
//!
//! Everything that defines what a valid composition *is* lives here: the
//! serde model, the note notation, the validation rules and the supported
//! format versions. This crate is pure: no file I/O, no audio.
//!
//! - [`parse_composition`] turns `.bm1` JSON text into a validated
//!   [`model::Composition`] (sample defaults applied).
//! - [`to_json`] serializes a composition back to JSON text (used by the
//!   editor).

pub mod model;
pub mod note;
pub mod validate;

use thiserror::Error;

pub use model::{Arrangement, Composition, KeyValue, Metadata, Pattern, Sample, Track};
pub use validate::{ValidationError, SUPPORTED_FORMAT_VERSIONS};

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("the JSON does not have the expected shape: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("invalid composition: {0}")]
    Validation(#[from] ValidationError),
}

/// Parses `.bm1` JSON text: deserializes it, applies the sample defaults
/// (`metadata.others` or `C4`), then validates the result.
///
/// Sample `file` paths are left exactly as written; resolving them against
/// the composition's location is the job of `bm-project`.
pub fn parse_composition(text: &str) -> Result<Composition, FormatError> {
    let mut composition: Composition = serde_json::from_str(text)?;
    validate::apply_sample_defaults(&mut composition)?;
    validate::validate(&composition)?;
    Ok(composition)
}

/// Serializes a composition to pretty-printed JSON text.
pub fn to_json(composition: &Composition) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(composition)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
        "metadata": { "version": "1.0", "title": "t", "bpm": 120,
            "others": [ { "key": "sampleDefaultNote", "value": "E" },
                        { "key": "sampleDefaultOctave", "value": "3" } ] },
        "samples": [ { "id": "s", "file": "s.wav" },
                     { "id": "t", "file": "t.wav", "rootNote": "A", "rootOctave": 5 } ],
        "patterns": [ { "id": "p", "sample": "s", "steps": ["C4", null, null, null] } ],
        "arrangement": { "tracks": [ { "id": "tr", "sequence": ["p", null] } ] }
    }"#;

    #[test]
    fn parse_applies_defaults_from_metadata_others() {
        let c = parse_composition(MINIMAL).unwrap();
        assert_eq!(c.samples[0].root_note.as_deref(), Some("E"));
        assert_eq!(c.samples[0].root_octave, Some(3));
        // explicit values are kept
        assert_eq!(c.samples[1].root_note.as_deref(), Some("A"));
        assert_eq!(c.samples[1].root_octave, Some(5));
    }

    #[test]
    fn parse_reports_shape_errors_and_validation_errors_separately() {
        assert!(matches!(
            parse_composition("{ not json"),
            Err(FormatError::Parse(_))
        ));
        let bad_ref = MINIMAL.replace("\"sample\": \"s\"", "\"sample\": \"nope\"");
        assert!(matches!(
            parse_composition(&bad_ref),
            Err(FormatError::Validation(ValidationError::UnknownSampleReference { .. }))
        ));
    }

    #[test]
    fn serialize_then_parse_round_trips() {
        let original = parse_composition(MINIMAL).unwrap();
        let text = to_json(&original).unwrap();
        let again = parse_composition(&text).unwrap();
        assert_eq!(again.metadata.title, original.metadata.title);
        assert_eq!(again.patterns[0].steps, original.patterns[0].steps);
        assert_eq!(again.arrangement.tracks[0].sequence, original.arrangement.tracks[0].sequence);
        assert_eq!(again.samples[0].root_note, original.samples[0].root_note);
    }
}
