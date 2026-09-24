//! Domain model of the bit-music composition format.
//!
//! JSON structure:
//! - `metadata`: descriptive information about the composition (title, bpm, ...).
//! - `samples[]`: an audio sample + the note/octave it was recorded at
//!   (root note/octave), needed to pitch-shift it to other notes.
//! - `patterns[]`: reusable "chunks" (sample + octave + note sequence).
//! - `arrangement.tracks[]`: parallel tracks, each chaining `patterns` by id.

use serde::{Deserialize, Serialize};

/// Full composition as deserialized from the input JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Composition {
    pub metadata: Metadata,
    pub samples: Vec<Sample>,
    pub patterns: Vec<Pattern>,
    pub arrangement: Arrangement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub version: String,
    pub title: String,
    pub bpm: u32,
    /// How many `pattern.steps` fit in one beat. Defaults to 4 (each step
    /// is a sixteenth note in 4/4). See `resolve::seconds_per_step`.
    #[serde(default = "default_steps_per_beat")]
    pub steps_per_beat: u32,
    #[serde(default)]
    pub others: Vec<KeyValue>,
}

fn default_steps_per_beat() -> u32 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

/// An audio sample and the note/octave it was originally recorded at (root
/// note / root octave), so the engine knows how much pitch-shift to apply
/// when playing it at any other note.
///
/// `root_note`/`root_octave` are optional in the JSON: if omitted, they are
/// filled in from `metadata.others["sampleDefaultNote"/"sampleDefaultOctave"]`
/// (or "C"/4 if those aren't there either) during loading — see
/// `loader::load_composition`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub id: String,
    /// Path to the `.wav`, absolute or relative. If relative (including
    /// just a bare file name, no directory), it is resolved against the
    /// directory of the `.bm1` file itself — see `loader::load_composition`.
    /// After loading, this field is already resolved to a usable path.
    pub file: String,
    /// Note name without octave, e.g. "C", "C#", "Eb".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_octave: Option<u8>,
}

/// A reusable "chunk": a sample (referenced by id) played following a
/// sequence of notes (or silences).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub id: String,
    /// id of a `Sample` in `Composition::samples`.
    pub sample: String,
    /// Each element is a full note with octave (e.g. "C4", "C#4", "Db3"; the
    /// older "C4#" and "D3b" are still read)
    /// or `null` (silence). Length must be a multiple of 4 (validated in
    /// `validate`).
    pub steps: Vec<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arrangement {
    pub tracks: Vec<Track>,
}

/// A track that plays in parallel with the others, chaining patterns by id.
/// `sequence[i] == None` represents a gap (silence) at that position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub sequence: Vec<Option<String>>,
}
