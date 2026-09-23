//! Computes the pitch-shift needed between the note a sample was recorded
//! at (`rootNote`/`rootOctave`) and the note a step asks for.

use bm_dsp::semitones_to_ratio;
use bm_format::model::Sample;
use bm_format::note::{self, Note};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchShift {
    /// Difference in semitones (can be negative: note lower than the root).
    pub semitones: i32,
    /// Resampling ratio to apply (see `audio::resample`).
    /// `1.0` == no change, `2.0` == one octave up, `0.5` == one octave down.
    pub ratio: f64,
}

/// Computes the pitch-shift to play `sample` at `note`.
///
/// Precondition: `sample.root_note`/`root_octave` are already resolved
/// (not `None`) — guaranteed by `bm_format::parse_composition`, which applies
/// the `metadata.others` defaults before returning the composition.
pub fn pitch_shift_for(sample: &Sample, note: &Note) -> PitchShift {
    let root_note = sample
        .root_note
        .as_deref()
        .expect("sample.root_note already resolved by the loader");
    let root_octave = sample
        .root_octave
        .expect("sample.root_octave already resolved by the loader");

    let (root_letter, root_accidental) =
        note::parse_note_name(root_note).expect("sample.root_note already validated");

    let root_semitone = root_octave as i32 * 12 + note::semitone_offset(root_letter, root_accidental);
    let target_semitone = note.absolute_semitone();

    let semitones = target_semitone - root_semitone;
    let ratio = semitones_to_ratio(semitones);

    PitchShift { semitones, ratio }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_at(root_note: &str, root_octave: u8) -> Sample {
        Sample {
            id: "s".to_string(),
            file: "s.wav".to_string(),
            root_note: Some(root_note.to_string()),
            root_octave: Some(root_octave),
        }
    }

    #[test]
    fn same_note_as_root_has_no_shift() {
        let sample = sample_at("C", 4);
        let note = note::parse_note("C4").unwrap();
        let shift = pitch_shift_for(&sample, &note);
        assert_eq!(shift.semitones, 0);
        assert!((shift.ratio - 1.0).abs() < 1e-9);
    }

    #[test]
    fn one_octave_above_root_doubles_ratio() {
        let sample = sample_at("C", 4);
        let note = note::parse_note("C5").unwrap();
        let shift = pitch_shift_for(&sample, &note);
        assert_eq!(shift.semitones, 12);
        assert!((shift.ratio - 2.0).abs() < 1e-9);
    }

    #[test]
    fn one_octave_below_root_halves_ratio() {
        let sample = sample_at("C", 4);
        let note = note::parse_note("C3").unwrap();
        let shift = pitch_shift_for(&sample, &note);
        assert_eq!(shift.semitones, -12);
        assert!((shift.ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn root_with_accidental_is_taken_into_account() {
        // root = C#4, requested note = D4 -> 1 semitone up.
        let sample = sample_at("C#", 4);
        let note = note::parse_note("D4").unwrap();
        let shift = pitch_shift_for(&sample, &note);
        assert_eq!(shift.semitones, 1);
    }
}
