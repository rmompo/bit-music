//! Resolution engine: turns `Composition` (samples + patterns +
//! arrangement) into a flat per-track timeline, applying the grid/column
//! model described in `specs/format.md`:
//!
//! - `arrangement` is a grid: each position `i` of `sequence` is a column
//!   shared by all tracks.
//! - The duration of column `i` (in steps) is the longest `steps.len()`
//!   among the patterns occupying that position in any track. If every
//!   track has `null` there at once, the column lasts the minimum unit
//!   (4 steps).
//! - A track whose `sequence` is shorter than the total number of columns
//!   loops cyclically over its own sequence until it covers them all.
//!
//! The result is, for each track, a `Vec<Option<ResolvedStep>>` of length
//! `total_steps` (the same for every track, since columns are
//! synchronized) — `None` represents silence at that step.
//!
//! It also exposes the grid itself (`columns`) and, per track, the pattern
//! blocks (`clips`, including which ones are loop repetitions), which is
//! what a UI needs to draw the arrangement without re-implementing the
//! grid model.

use std::collections::HashMap;

use bm_format::model::{Composition, Metadata, Pattern, Track};
use bm_format::note::{self, Note};

/// Duration of a column when no track plays anything in it.
const FALLBACK_COLUMN_STEPS: usize = 4;

/// An already-resolved step: which sample plays and at what note.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStep {
    pub sample_id: String,
    pub note: Note,
}

/// One column of the shared arrangement grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    /// First step of the column on the global timeline.
    pub start_step: usize,
    /// Duration of the column in steps.
    pub len_steps: usize,
}

/// A pattern placed on a track: what a UI draws as one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub pattern_id: String,
    /// Index of the column the clip sits in.
    pub column: usize,
    /// First step of the clip on the global timeline.
    pub start_step: usize,
    /// Length of the pattern in steps (can be shorter than its column; the
    /// rest of the column is silence).
    pub len_steps: usize,
    /// `true` when this slot exists only because the track's `sequence`
    /// looped to cover the arrangement (not written in the file).
    pub is_repeat: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedTrack {
    pub id: String,
    /// One element per step (length == `ResolvedArrangement::total_steps`).
    /// `None` = silence.
    pub steps: Vec<Option<ResolvedStep>>,
    /// Pattern blocks on this track, in timeline order. Gaps (`null`
    /// slots) produce no clip.
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone)]
pub struct ResolvedArrangement {
    pub tracks: Vec<ResolvedTrack>,
    /// The shared grid, one entry per column.
    pub columns: Vec<Column>,
    pub total_steps: usize,
}

/// Resolves the `arrangement` of an already-validated composition (assumes
/// every note and reference is valid — `bm_format::validate::validate` must
/// have succeeded beforehand).
pub fn resolve(composition: &Composition) -> ResolvedArrangement {
    let patterns: HashMap<&str, &Pattern> = composition
        .patterns
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let tracks = &composition.arrangement.tracks;

    let max_columns = tracks.iter().map(|t| t.sequence.len()).max().unwrap_or(0);
    let column_len = compute_column_lengths(tracks, &patterns, max_columns);
    let total_steps: usize = column_len.iter().sum();

    let mut columns = Vec::with_capacity(column_len.len());
    let mut offset = 0;
    for &len in &column_len {
        columns.push(Column {
            start_step: offset,
            len_steps: len,
        });
        offset += len;
    }

    let resolved_tracks = tracks
        .iter()
        .map(|track| {
            let (steps, clips) = resolve_track(track, &patterns, &columns, total_steps);
            ResolvedTrack {
                id: track.id.clone(),
                steps,
                clips,
            }
        })
        .collect();

    ResolvedArrangement {
        tracks: resolved_tracks,
        columns,
        total_steps,
    }
}

/// Real duration (in seconds) of a single step, derived from
/// `metadata.bpm` and `metadata.stepsPerBeat`:
/// `secondsPerStep = (60 / bpm) / stepsPerBeat`.
///
/// Assumes `bpm > 0` and `steps_per_beat > 0` (guaranteed by
/// `validate::validate`).
pub fn seconds_per_step(metadata: &Metadata) -> f64 {
    let seconds_per_beat = 60.0 / metadata.bpm as f64;
    seconds_per_beat / metadata.steps_per_beat as f64
}

/// Duration (in steps) of each of the `max_columns` grid columns, looking
/// at which pattern (the longest one) occupies each position across any
/// track, with cyclic looping for shorter sequences.
fn compute_column_lengths(
    tracks: &[Track],
    patterns: &HashMap<&str, &Pattern>,
    max_columns: usize,
) -> Vec<usize> {
    let mut column_len = vec![0usize; max_columns];

    for track in tracks {
        if track.sequence.is_empty() {
            continue;
        }
        for (col, slot_len) in column_len.iter_mut().enumerate() {
            let slot = &track.sequence[col % track.sequence.len()];
            if let Some(pattern_id) = slot {
                // The reference was already validated in `validate::validate`.
                let pattern_len = patterns[pattern_id.as_str()].steps.len();
                *slot_len = (*slot_len).max(pattern_len);
            }
        }
    }

    for len in &mut column_len {
        if *len == 0 {
            *len = FALLBACK_COLUMN_STEPS;
        }
    }

    column_len
}

/// Expands a track into its flat step timeline and its clips, with cyclic
/// looping over its `sequence` and silence padding up to each column's
/// duration.
fn resolve_track(
    track: &Track,
    patterns: &HashMap<&str, &Pattern>,
    columns: &[Column],
    total_steps: usize,
) -> (Vec<Option<ResolvedStep>>, Vec<Clip>) {
    let mut steps = Vec::with_capacity(total_steps);
    let mut clips = Vec::new();

    if track.sequence.is_empty() {
        steps.resize_with(total_steps, || None);
        return (steps, clips);
    }

    for (col, column) in columns.iter().enumerate() {
        let slot = &track.sequence[col % track.sequence.len()];
        match slot {
            Some(pattern_id) => {
                let pattern = patterns[pattern_id.as_str()];
                clips.push(Clip {
                    pattern_id: pattern_id.clone(),
                    column: col,
                    start_step: column.start_step,
                    len_steps: pattern.steps.len(),
                    is_repeat: col >= track.sequence.len(),
                });
                for i in 0..column.len_steps {
                    let resolved = pattern
                        .steps
                        .get(i)
                        .and_then(|s| s.as_ref())
                        .map(|raw_note| ResolvedStep {
                            sample_id: pattern.sample.clone(),
                            // The note was already validated by bm-format.
                            note: note::parse_note(raw_note).expect("note already validated"),
                        });
                    steps.push(resolved);
                }
            }
            None => steps.extend(std::iter::repeat_with(|| None).take(column.len_steps)),
        }
    }

    (steps, clips)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_format::model::{Arrangement, Sample};

    fn pattern(id: &str, sample: &str, steps: &[Option<&str>]) -> Pattern {
        Pattern {
            id: id.to_string(),
            sample: sample.to_string(),
            steps: steps.iter().map(|s| s.map(str::to_string)).collect(),
        }
    }

    fn track(id: &str, sequence: &[Option<&str>]) -> Track {
        Track {
            id: id.to_string(),
            sequence: sequence.iter().map(|s| s.map(str::to_string)).collect(),
        }
    }

    fn sample(id: &str) -> Sample {
        Sample {
            id: id.to_string(),
            file: format!("{id}.wav"),
            root_note: Some("C".to_string()),
            root_octave: Some(4),
        }
    }

    fn composition(patterns: Vec<Pattern>, tracks: Vec<Track>) -> Composition {
        Composition {
            metadata: Metadata {
                version: "1.0".to_string(),
                title: "test".to_string(),
                bpm: 120,
                steps_per_beat: 4,
                others: vec![],
            },
            samples: vec![sample("kick"), sample("lead")],
            patterns,
            arrangement: Arrangement { tracks },
        }
    }

    #[test]
    fn seconds_per_step_at_120bpm_default_steps_per_beat() {
        let metadata = Metadata {
            version: "1.0".to_string(),
            title: "test".to_string(),
            bpm: 120,
            steps_per_beat: 4,
            others: vec![],
        };
        // 120 bpm -> 0.5s per beat -> 0.125s per step with stepsPerBeat=4.
        assert!((seconds_per_step(&metadata) - 0.125).abs() < f64::EPSILON);
    }

    #[test]
    fn column_duration_is_the_longest_pattern_at_that_position() {
        // 4 steps vs 8 steps at the same column -> the column lasts 8.
        let comp = composition(
            vec![
                pattern("short", "kick", &[Some("C4"), None, Some("C4"), None]),
                pattern(
                    "long",
                    "lead",
                    &[
                        Some("C4"),
                        None,
                        None,
                        None,
                        Some("C4"),
                        None,
                        None,
                        None,
                    ],
                ),
            ],
            vec![
                track("a", &[Some("short")]),
                track("b", &[Some("long")]),
            ],
        );

        let resolved = resolve(&comp);
        assert_eq!(resolved.total_steps, 8);
        assert_eq!(resolved.tracks[0].steps.len(), 8);
        assert_eq!(resolved.tracks[1].steps.len(), 8);
        // The short pattern is padded with silence up to the column duration.
        assert!(resolved.tracks[0].steps[4].is_none());
        assert!(resolved.tracks[0].steps[5].is_none());
    }

    #[test]
    fn shorter_sequence_loops_to_cover_all_columns() {
        let comp = composition(
            vec![pattern("p", "kick", &[Some("C4"), None, None, None])],
            vec![
                track("a", &[Some("p")]), // 1 column
                track("b", &[Some("p"), Some("p"), Some("p")]), // 3 columns -> wins
            ],
        );

        let resolved = resolve(&comp);
        // 3 columns of 4 steps = 12
        assert_eq!(resolved.total_steps, 12);
        // track "a" loops 3 times over its single column.
        let a = &resolved.tracks[0].steps;
        assert!(a[0].is_some());
        assert!(a[4].is_some());
        assert!(a[8].is_some());
    }

    #[test]
    fn simultaneous_null_column_falls_back_to_four_steps() {
        let comp = composition(
            vec![pattern("p", "kick", &[Some("C4"), None, None, None])],
            vec![
                track("a", &[Some("p"), None]),
                track("b", &[None, Some("p")]),
            ],
        );

        let resolved = resolve(&comp);
        // 2 columns, both of 4 steps (the first via "p", the second via
        // fallback since at column 1 "a" is null but "b" has "p"... here no
        // column is actually 100% null, so we also test the real fallback
        // case with a third, fully empty track.
        assert_eq!(resolved.total_steps, 8);

        let comp_all_null = composition(
            vec![pattern("p", "kick", &[Some("C4"), None, None, None])],
            vec![track("a", &[None])],
        );
        let resolved_all_null = resolve(&comp_all_null);
        assert_eq!(resolved_all_null.total_steps, FALLBACK_COLUMN_STEPS);
    }

    #[test]
    fn trailing_null_extends_the_longest_track_and_loop_gap_for_shorter_ones() {
        let comp = composition(
            vec![pattern("p", "kick", &[Some("C4"), None, None, None])],
            vec![
                track("longest", &[Some("p"), None]), // 2 columns, the 2nd is trailing silence
                track("shorter", &[Some("p")]),        // loops, but there are only 2 columns
            ],
        );

        let resolved = resolve(&comp);
        assert_eq!(resolved.total_steps, 8); // 2 columns x 4 steps
        // The "longest" track has real silence in the 2nd column (indices 4-7).
        assert!(resolved.tracks[0].steps[4..8].iter().all(Option::is_none));
        // The "shorter" track loops: it plays again in the 2nd column.
        assert!(resolved.tracks[1].steps[4].is_some());
    }

    #[test]
    fn columns_and_clips_describe_the_grid() {
        let comp = composition(
            vec![
                pattern("short", "kick", &[Some("C4"), None, None, None]),
                pattern("long", "lead", &[Some("C4"), None, None, None, None, None, None, None]),
            ],
            vec![
                track("a", &[Some("short"), None, Some("short")]),
                track("b", &[Some("long")]),
            ],
        );

        let r = resolve(&comp);
        // col0: max(4, 8) = 8 ; col1: nobody but track b loops "long" -> 8 ;
        // col2: max(4, 8 via loop) = 8
        assert_eq!(
            r.columns,
            vec![
                Column { start_step: 0, len_steps: 8 },
                Column { start_step: 8, len_steps: 8 },
                Column { start_step: 16, len_steps: 8 },
            ]
        );
        assert_eq!(r.total_steps, 24);

        // track a: clips only where the slot is not null; short pattern
        // keeps its own 4-step length inside an 8-step column.
        let a = &r.tracks[0].clips;
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].column, a[0].start_step, a[0].len_steps, a[0].is_repeat), (0, 0, 4, false));
        assert_eq!((a[1].column, a[1].start_step, a[1].len_steps, a[1].is_repeat), (2, 16, 4, false));

        // track b: written once, looped for columns 1 and 2.
        let b = &r.tracks[1].clips;
        assert_eq!(b.len(), 3);
        assert_eq!(b.iter().map(|c| c.is_repeat).collect::<Vec<_>>(), vec![false, true, true]);
    }
}
