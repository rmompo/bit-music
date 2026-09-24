//! The pattern "grid": a piano-roll style view of a pattern's steps.
//!
//! [`PatternGrid`] is the pure model (which pitches a pattern uses and at
//! which steps they sound); the `draw_*` functions turn it into pixels,
//! both as the full labeled grid of the properties panel and as the tiny
//! preview drawn inside an arrangement block.
//!
//! The properties grid is a piano roll like a DAW's: every semitone from the
//! start of the lowest octave the pattern uses to the end of the highest one,
//! highest pitch on top, with a keyboard on the left and the rows of the
//! black keys shaded. (The preview in a block stays compact: only the pitches
//! that are used.)

use std::collections::BTreeMap;

use bm_format::note::parse_note;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

/// One sounding step of the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridNote {
    pub step: usize,
    /// Index into [`PatternGrid::rows`] (0 = highest pitch).
    pub row: usize,
}

/// One row of the full (chromatic) grid: a semitone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PitchRow {
    /// `C4`, `C#4`, ... (sharps, the accidental before the octave, as DAWs
    /// write it).
    pub label: String,
    /// A black key of the piano.
    pub black: bool,
}

const PITCH_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

impl PitchRow {
    fn from_semitone(semitone: i32) -> Self {
        let name = PITCH_NAMES[semitone.rem_euclid(12) as usize];
        Self {
            label: format!("{name}{}", semitone.div_euclid(12)),
            black: name.contains('#'),
        }
    }
}

/// A pattern reduced to what a grid needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternGrid {
    pub len_steps: usize,
    /// Distinct pitches used, highest first, labeled as written in the file
    /// (`C4`, `E#4` or the older `E4#`, ...). Enharmonic spellings share a row.
    pub rows: Vec<String>,
    pub notes: Vec<GridNote>,
    /// The full chromatic range, highest first: from the start of the lowest
    /// octave used to the end of the highest one. Empty for a silent pattern.
    pub chromatic: Vec<PitchRow>,
    /// The same sounding steps, with `row` indexing [`Self::chromatic`].
    pub chromatic_notes: Vec<GridNote>,
}

impl PatternGrid {
    pub fn from_steps(steps: &[Option<String>]) -> Self {
        // absolute semitone -> label (first spelling seen wins)
        let mut pitches: BTreeMap<i32, String> = BTreeMap::new();
        let mut sounding: Vec<(usize, i32)> = Vec::new();

        for (step, raw) in steps.iter().enumerate() {
            let Some(raw) = raw else { continue };
            // Notes were validated on load; an unparsable one is just skipped.
            let Ok(note) = parse_note(raw) else { continue };
            let semitone = note.absolute_semitone();
            pitches.entry(semitone).or_insert_with(|| raw.clone());
            sounding.push((step, semitone));
        }

        // The chromatic range: whole octaves around what is used.
        let (chromatic, chromatic_notes) = match (pitches.keys().next(), pitches.keys().next_back()) {
            (Some(&low), Some(&high)) => {
                let bottom = low.div_euclid(12) * 12;
                let top = high.div_euclid(12) * 12 + 11;
                let rows = (bottom..=top).rev().map(PitchRow::from_semitone).collect();
                let notes = sounding
                    .iter()
                    .map(|&(step, semitone)| GridNote { step, row: (top - semitone) as usize })
                    .collect();
                (rows, notes)
            }
            _ => (Vec::new(), Vec::new()),
        };

        let ordered: Vec<i32> = pitches.keys().rev().copied().collect();
        let rows = ordered.iter().map(|s| pitches[s].clone()).collect();
        let notes = sounding
            .into_iter()
            .map(|(step, semitone)| GridNote {
                step,
                row: ordered.iter().position(|s| *s == semitone).unwrap_or(0),
            })
            .collect();

        Self {
            len_steps: steps.len(),
            rows,
            notes,
            chromatic,
            chromatic_notes,
        }
    }
}

pub const CELL_WIDTH: f32 = 20.0;
pub const CELL_HEIGHT: f32 = 16.0;
const LABEL_WIDTH: f32 = 48.0;
const HEADER_HEIGHT: f32 = 16.0;

/// Draws the full grid, as a DAW's piano roll: a keyboard with the note names
/// on the left (white and black keys), step numbers on top, beat shading,
/// darker rows for the black keys, and a filled cell for every sounding step.
pub fn draw_pattern_grid(ui: &mut egui::Ui, grid: &PatternGrid, steps_per_beat: usize, color: Color32) {
    let rows = grid.chromatic.len().max(1);
    let size = Vec2::new(
        LABEL_WIDTH + grid.len_steps as f32 * CELL_WIDTH,
        HEADER_HEIGHT + rows as f32 * CELL_HEIGHT,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let text_color = visuals.text_color();
    let line = Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);
    let beat = steps_per_beat.max(1);
    let origin = rect.min + Vec2::new(LABEL_WIDTH, HEADER_HEIGHT);

    // Background: alternate shading per beat.
    for step in 0..grid.len_steps {
        let shade = if (step / beat) % 2 == 0 {
            visuals.extreme_bg_color
        } else {
            visuals.faint_bg_color
        };
        let x = origin.x + step as f32 * CELL_WIDTH;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x, origin.y), Vec2::new(CELL_WIDTH, rows as f32 * CELL_HEIGHT)),
            0.0,
            shade,
        );
    }

    // Rows: darker bands for the black keys, a line between rows, and the
    // keyboard on the left with the name of each note.
    for (i, row) in grid.chromatic.iter().enumerate() {
        let y = origin.y + i as f32 * CELL_HEIGHT;
        if !row.black {
            // The rows of the white keys are a little lighter than those of
            // the black keys (the same as the darker black-key rows of a DAW,
            // but readable on a dark theme too).
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(origin.x, y),
                    Vec2::new(grid.len_steps as f32 * CELL_WIDTH, CELL_HEIGHT),
                ),
                0.0,
                Color32::from_white_alpha(16),
            );
        }
        painter.line_segment([Pos2::new(origin.x, y), Pos2::new(rect.max.x, y)], line);

        // A white key fills the label column; a black key is a shorter, dark
        // key over it, as on a piano.
        let key = Rect::from_min_size(
            Pos2::new(rect.min.x, y),
            Vec2::new(if row.black { LABEL_WIDTH * 0.72 } else { LABEL_WIDTH - 2.0 }, CELL_HEIGHT - 1.0),
        );
        let (fill, ink) = if row.black {
            (Color32::BLACK, Color32::from_gray(215))
        } else {
            (Color32::from_gray(212), Color32::from_gray(30))
        };
        painter.rect_filled(key, 2.0, fill);
        if row.black {
            painter.rect_stroke(key, 2.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
        }
        painter.text(
            Pos2::new(key.max.x - 4.0, y + CELL_HEIGHT / 2.0),
            egui::Align2::RIGHT_CENTER,
            &row.label,
            egui::FontId::monospace(10.0),
            ink,
        );
    }

    // Beat lines and step numbers (one number per beat).
    for step in (0..=grid.len_steps).step_by(beat) {
        let x = origin.x + step as f32 * CELL_WIDTH;
        painter.line_segment([Pos2::new(x, rect.min.y + HEADER_HEIGHT - 4.0), Pos2::new(x, rect.max.y)], line);
        if step < grid.len_steps {
            painter.text(
                Pos2::new(x + 2.0, rect.min.y + 1.0),
                egui::Align2::LEFT_TOP,
                (step + 1).to_string(),
                egui::FontId::proportional(10.0),
                text_color,
            );
        }
    }

    // Sounding steps.
    for note in &grid.chromatic_notes {
        let min = Pos2::new(
            origin.x + note.step as f32 * CELL_WIDTH + 1.0,
            origin.y + note.row as f32 * CELL_HEIGHT + 1.0,
        );
        painter.rect_filled(
            Rect::from_min_size(min, Vec2::new(CELL_WIDTH - 2.0, CELL_HEIGHT - 2.0)),
            3.0,
            color,
        );
    }
}

/// The rectangles of the notes of the small preview inside `rect` (an
/// arrangement block), one per sounding step, in the same order as
/// `grid.chromatic_notes`.
///
/// It uses the same full chromatic scale as the properties grid (every
/// semitone of the octaves the pattern uses), so a note's height means the
/// same in both places: the highest pitch at the top, the lowest at the bottom.
pub fn mini_note_rects(rect: Rect, grid: &PatternGrid) -> Vec<Rect> {
    if grid.len_steps == 0 || grid.chromatic.is_empty() {
        return Vec::new();
    }
    let rows = grid.chromatic.len() as f32;
    let step_w = rect.width() / grid.len_steps as f32;
    let row_h = rect.height() / rows;

    grid.chromatic_notes
        .iter()
        .map(|note| {
            let min = Pos2::new(
                rect.min.x + note.step as f32 * step_w,
                rect.min.y + note.row as f32 * row_h,
            );
            Rect::from_min_size(min, Vec2::new((step_w - 1.0).max(1.0), (row_h - 0.5).max(1.0)))
        })
        .collect()
}

/// Draws the small preview of the pattern inside `rect` (an arrangement
/// block): one thin bar per sounding step, on the full chromatic scale.
pub fn draw_mini_notes(painter: &egui::Painter, rect: Rect, grid: &PatternGrid, color: Color32) {
    for note in mini_note_rects(rect, grid) {
        painter.rect_filled(note, 1.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(items: &[Option<&str>]) -> Vec<Option<String>> {
        items.iter().map(|s| s.map(str::to_string)).collect()
    }

    #[test]
    fn rows_are_distinct_pitches_highest_first() {
        let g = PatternGrid::from_steps(&steps(&[Some("C4"), Some("E4"), None, Some("C4")]));
        assert_eq!(g.len_steps, 4);
        assert_eq!(g.rows, vec!["E4", "C4"]);
        assert_eq!(
            g.notes,
            vec![
                GridNote { step: 0, row: 1 },
                GridNote { step: 1, row: 0 },
                GridNote { step: 3, row: 1 },
            ]
        );
    }

    #[test]
    fn enharmonic_spellings_share_a_row() {
        let g = PatternGrid::from_steps(&steps(&[Some("C4#"), Some("D4b")]));
        assert_eq!(g.rows.len(), 1);
        assert_eq!(g.rows[0], "C4#");
        assert_eq!(g.notes.len(), 2);
    }

    #[test]
    fn octaves_order_above_letters() {
        let g = PatternGrid::from_steps(&steps(&[Some("B3"), Some("C5"), Some("A4")]));
        assert_eq!(g.rows, vec!["C5", "A4", "B3"]);
    }

    #[test]
    fn a_silent_pattern_has_no_rows_or_notes() {
        let g = PatternGrid::from_steps(&steps(&[None, None, None, None]));
        assert!(g.rows.is_empty() && g.notes.is_empty());
        assert_eq!(g.len_steps, 4);
    }

    #[test]
    fn the_chromatic_range_covers_whole_octaves_highest_first() {
        // C4 to G4: the whole of octave 4, from B4 down to C4.
        let g = PatternGrid::from_steps(&steps(&[Some("C4"), Some("G4"), Some("F4#")]));
        let labels: Vec<&str> = g.chromatic.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            ["B4", "A#4", "A4", "G#4", "G4", "F#4", "F4", "E4", "D#4", "D4", "C#4", "C4"]
        );
        let black: Vec<bool> = g.chromatic.iter().map(|r| r.black).collect();
        assert_eq!(
            black,
            [false, true, false, true, false, true, false, false, true, false, true, false]
        );
        // Each sounding step points at its own row.
        let rows: Vec<(usize, &str)> = g
            .chromatic_notes
            .iter()
            .map(|n| (n.step, g.chromatic[n.row].label.as_str()))
            .collect();
        assert_eq!(rows, [(0, "C4"), (1, "G4"), (2, "F#4")]);
    }

    #[test]
    fn several_octaves_are_all_included_and_flats_land_on_the_sharp_row() {
        let g = PatternGrid::from_steps(&steps(&[Some("D3b"), Some("E4")]));
        // D3b is C#3: octaves 3 and 4, from B4 down to C3.
        assert_eq!(g.chromatic.len(), 24);
        assert_eq!(g.chromatic.first().unwrap().label, "B4");
        assert_eq!(g.chromatic.last().unwrap().label, "C3");
        assert_eq!(g.chromatic[g.chromatic_notes[0].row].label, "C#3");
        // The compact rows (used pitches, for the preview and the counts) are unchanged.
        assert_eq!(g.rows.len(), 2);
    }

    #[test]
    fn the_block_preview_uses_the_same_full_scale_as_the_grid() {
        let g = PatternGrid::from_steps(&steps(&[Some("C4"), Some("G4"), Some("F#4"), Some("C4")]));
        let area = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(120.0, 36.0));
        let rects = mini_note_rects(area, &g);
        assert_eq!(rects.len(), 4);
        // 12 rows (the whole octave 4) share the height: 3 points each.
        let row_h = 36.0 / 12.0;
        for (note, rect) in g.chromatic_notes.iter().zip(&rects) {
            assert!((rect.min.y - (20.0 + note.row as f32 * row_h)).abs() < 1e-4);
            assert!(rect.min.y >= area.min.y && rect.max.y <= area.max.y + 1e-4);
        }
        // Pitch order: G4 above F#4 above C4; the two C4 at the same height.
        let y = |i: usize| rects[i].min.y;
        assert!(y(1) < y(2) && y(2) < y(0));
        assert_eq!(y(0), y(3));
        // Steps go left to right.
        assert!(rects[0].min.x < rects[1].min.x && rects[1].min.x < rects[2].min.x);
    }

    #[test]
    fn a_silent_pattern_has_no_chromatic_rows() {
        let g = PatternGrid::from_steps(&steps(&[None, None]));
        assert!(g.chromatic.is_empty() && g.chromatic_notes.is_empty());
    }

    #[test]
    fn grid_and_mini_preview_draw_without_panicking() {
        let g = PatternGrid::from_steps(&steps(&[Some("C4"), None, Some("E4"), None]));
        egui::__run_test_ui(|ui| {
            draw_pattern_grid(ui, &g, 4, Color32::LIGHT_BLUE);
            let painter = ui.painter().clone();
            draw_mini_notes(&painter, Rect::from_min_size(Pos2::ZERO, Vec2::new(80.0, 30.0)), &g, Color32::WHITE);
        });
    }
}
