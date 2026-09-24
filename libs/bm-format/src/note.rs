//! Parsing and validation of bit-music musical notation.
//!
//! Two formats coexist in the format:
//! - **Full note** (used in `pattern.steps`): `<letter A-G><accidental?><octave 0-8>`,
//!   the way DAWs write it, e.g. `"C4"`, `"C#4"`, `"Db3"`. The older order
//!   with the accidental after the octave (`"C4#"`, `"D3b"`) is still
//!   accepted when reading.
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
    #[error("invalid note format: '{0}' (expected NOTE[#|b]OCTAVE, e.g. 'C4', 'C#4', 'Db3')")]
    InvalidFormat(String),
    #[error("invalid note letter '{0}' (must be A-G)")]
    InvalidLetter(char),
    #[error("octave out of range (0-8): {0}")]
    OctaveOutOfRange(u8),
}

/// Parses a full note with octave, e.g. as used in `pattern.steps`.
///
/// The canonical spelling puts the accidental before the octave (`C#4`,
/// `Db3`, as in a DAW). The older one, with the accidental after the octave
/// (`C4#`, `D3b`), is accepted too. Letters are case-insensitive, and `b` or
/// `B` after the letter or the octave mean flat.
pub fn parse_note(input: &str) -> Result<Note, NoteParseError> {
    let invalid = || NoteParseError::InvalidFormat(input.to_string());
    let accidental_of = |c: char| match c {
        '#' => Some(Accidental::Sharp),
        'b' | 'B' => Some(Accidental::Flat),
        _ => None,
    };

    let mut chars = input.chars().peekable();
    let letter = chars.next().ok_or_else(invalid)?.to_ascii_uppercase();
    if !('A'..='G').contains(&letter) {
        return Err(NoteParseError::InvalidLetter(letter));
    }

    // Canonical: the accidental right after the letter.
    let mut accidental = chars.peek().copied().and_then(accidental_of);
    if accidental.is_some() {
        chars.next();
    }

    let mut digits = String::new();
    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
        digits.push(c);
        chars.next();
    }
    if digits.is_empty() {
        return Err(invalid());
    }
    let octave: u8 = digits.parse().map_err(|_| invalid())?;
    if octave > 8 {
        return Err(NoteParseError::OctaveOutOfRange(octave));
    }

    // Older spelling: the accidental after the octave (only if there was none
    // before it).
    if let Some(c) = chars.next() {
        match accidental_of(c) {
            Some(a) if accidental.is_none() => accidental = Some(a),
            _ => return Err(invalid()),
        }
    }
    if chars.next().is_some() {
        return Err(invalid());
    }

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

impl std::fmt::Display for Note {
    /// The canonical spelling, the accidental before the octave: `C#4`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let accidental = match self.accidental {
            None => "",
            Some(Accidental::Sharp) => "#",
            Some(Accidental::Flat) => "b",
        };
        write!(f, "{}{accidental}{}", self.letter, self.octave)
    }
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
    fn the_accidental_goes_before_the_octave_like_in_a_daw() {
        let note = |letter, octave, accidental| Note { letter, octave, accidental };
        assert_eq!(parse_note("C#4").unwrap(), note('C', 4, Some(Accidental::Sharp)));
        assert_eq!(parse_note("Db3").unwrap(), note('D', 3, Some(Accidental::Flat)));
        assert_eq!(parse_note("bb2").unwrap(), note('B', 2, Some(Accidental::Flat)));
        assert_eq!(parse_note("g#8").unwrap(), note('G', 8, Some(Accidental::Sharp)));
        assert_eq!(parse_note("EB5").unwrap(), note('E', 5, Some(Accidental::Flat)));
    }

    #[test]
    fn the_older_order_with_the_accidental_after_the_octave_still_reads() {
        // Both spellings are the same note.
        for (old, new) in [("C4#", "C#4"), ("D3b", "Db3"), ("g8#", "G#8"), ("B2b", "Bb2"), ("A4", "A4")] {
            assert_eq!(parse_note(old).unwrap(), parse_note(new).unwrap(), "{old} vs {new}");
        }
    }

    #[test]
    fn notes_print_in_the_canonical_spelling() {
        for (input, canonical) in [("C4#", "C#4"), ("D3b", "Db3"), ("e5", "E5"), ("F#4", "F#4")] {
            assert_eq!(parse_note(input).unwrap().to_string(), canonical);
        }
    }

    #[test]
    fn rejects_invalid_notes() {
        assert!(parse_note("H4").is_err());
        assert!(parse_note("C9").is_err());
        assert!(parse_note("C").is_err());
        assert!(parse_note("C4##").is_err());
        // The accidental only once, and only in one place.
        assert!(parse_note("C#4#").is_err());
        assert!(parse_note("C##4").is_err());
        assert!(parse_note("Cb4b").is_err());
        assert!(parse_note("C#").is_err());
        assert!(parse_note("#4").is_err());
        assert!(parse_note("C4x").is_err());
        assert!(parse_note("C 4").is_err());
        assert!(parse_note("").is_err());
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
