//! Parsing and validation of bit-music musical notation.
//!
//! Two formats coexist in the format:
//! - **Full note** (used in `pattern.steps`): `<letter A-G><octave 0-8><accidental?>`
//!   e.g. `"C4"`, `"C4#"`, `"D3b"`.
//! - **Note name** (used in `sample.rootNote` and in the `metadata.others`
//!   defaults): just `<letter><accidental?>`, e.g. `"C"`, `"C#"`, `"Eb"`.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accidental {
    Sharp,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub letter: char,
    pub octave: u8,
    pub accidental: Option<Accidental>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NoteParseError {
    #[error("invalid note format: '{0}' (expected NOTE+OCTAVE[#|b], e.g. 'C4', 'D3b')")]
    InvalidFormat(String),
    #[error("invalid note letter '{0}' (must be A-G)")]
    InvalidLetter(char),
    #[error("octave out of range (0-8): {0}")]
    OctaveOutOfRange(u8),
}

/// Parses a full note with octave, e.g. as used in `pattern.steps`.
pub fn parse_note(input: &str) -> Result<Note, NoteParseError> {
    let mut chars = input.chars();
    let letter = chars
        .next()
        .ok_or_else(|| NoteParseError::InvalidFormat(input.to_string()))?
        .to_ascii_uppercase();

    if !('A'..='G').contains(&letter) {
        return Err(NoteParseError::InvalidLetter(letter));
    }

    let rest: String = chars.collect();
    let split_at = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let (digits, suffix) = rest.split_at(split_at);

    if digits.is_empty() {
        return Err(NoteParseError::InvalidFormat(input.to_string()));
    }
    let octave: u8 = digits
        .parse()
        .map_err(|_| NoteParseError::InvalidFormat(input.to_string()))?;
    if octave > 8 {
        return Err(NoteParseError::OctaveOutOfRange(octave));
    }

    let accidental = match suffix {
        "" => None,
        "#" => Some(Accidental::Sharp),
        "b" | "B" => Some(Accidental::Flat),
        _ => return Err(NoteParseError::InvalidFormat(input.to_string())),
    };

    Ok(Note {
        letter,
        octave,
        accidental,
    })
}

/// Validates a note name without octave, e.g. as used in `sample.rootNote`.
pub fn is_valid_note_name(input: &str) -> bool {
    parse_note_name(input).is_some()
}

/// Parses a note name without octave (letter + optional accidental), e.g.
/// as used in `sample.rootNote` and in the `metadata.others` defaults.
/// Returns `None` if the format is invalid.
pub fn parse_note_name(input: &str) -> Option<(char, Option<Accidental>)> {
    let mut chars = input.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if !('A'..='G').contains(&letter) {
        return None;
    }
    let accidental = match chars.next() {
        None => None,
        Some('#') => Some(Accidental::Sharp),
        Some('b') | Some('B') => Some(Accidental::Flat),
        Some(_) => return None,
    };
    if chars.next().is_some() {
        return None;
    }
    Some((letter, accidental))
}

/// Semitone offset (0-11) of a letter+accidental within an octave, taking C
/// as reference 0. Accidentals that fall outside the range (e.g. "Cb" or
/// "B#") are normalized within the octave via `rem_euclid`.
pub fn semitone_offset(letter: char, accidental: Option<Accidental>) -> i32 {
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => unreachable!("letter already validated as A-G"),
    };
    let delta = match accidental {
        None => 0,
        Some(Accidental::Sharp) => 1,
        Some(Accidental::Flat) => -1,
    };
    (base + delta).rem_euclid(12)
}

impl Note {
    /// Absolute semitone (octave included), useful for computing pitch
    /// differences between two notes. Not a standard MIDI number (there is
    /// no defined reference offset), just a consistent internal scale: all
    /// that matters is the *difference* between two calls to this method.
    pub fn absolute_semitone(&self) -> i32 {
        self.octave as i32 * 12 + semitone_offset(self.letter, self.accidental)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_notes() {
        assert_eq!(
            parse_note("C4").unwrap(),
            Note {
                letter: 'C',
                octave: 4,
                accidental: None
            }
        );
        assert_eq!(
            parse_note("D3b").unwrap(),
            Note {
                letter: 'D',
                octave: 3,
                accidental: Some(Accidental::Flat)
            }
        );
        assert_eq!(
            parse_note("g8#").unwrap(),
            Note {
                letter: 'G',
                octave: 8,
                accidental: Some(Accidental::Sharp)
            }
        );
    }

    #[test]
    fn rejects_invalid_notes() {
        assert!(parse_note("H4").is_err());
        assert!(parse_note("C9").is_err());
        assert!(parse_note("C").is_err());
        assert!(parse_note("C4##").is_err());
    }

    #[test]
    fn validates_note_names() {
        for n in ["C", "C#", "Eb", "g", "ab"] {
            assert!(is_valid_note_name(n), "'{n}' should be valid");
        }
        for n in ["H", "C4", "C##", ""] {
            assert!(!is_valid_note_name(n), "'{n}' should be invalid");
        }
    }

    #[test]
    fn semitone_offsets_match_standard_chromatic_scale() {
        assert_eq!(semitone_offset('C', None), 0);
        assert_eq!(semitone_offset('C', Some(Accidental::Sharp)), 1);
        assert_eq!(semitone_offset('D', Some(Accidental::Flat)), 1); // Db == C#
        assert_eq!(semitone_offset('A', None), 9);
        assert_eq!(semitone_offset('B', None), 11);
        // edge cases that cross an octave boundary within the offset (0-11)
        assert_eq!(semitone_offset('B', Some(Accidental::Sharp)), 0); // B# == C
        assert_eq!(semitone_offset('C', Some(Accidental::Flat)), 11); // Cb == B
    }

    #[test]
    fn absolute_semitone_distance_between_octaves() {
        let c4 = Note {
            letter: 'C',
            octave: 4,
            accidental: None,
        };
        let c5 = Note {
            letter: 'C',
            octave: 5,
            accidental: None,
        };
        // one octave == 12 semitones
        assert_eq!(c5.absolute_semitone() - c4.absolute_semitone(), 12);

        let e4 = Note {
            letter: 'E',
            octave: 4,
            accidental: None,
        };
        assert_eq!(e4.absolute_semitone() - c4.absolute_semitone(), 4);
    }
}
