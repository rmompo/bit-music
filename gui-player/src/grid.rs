//! The pattern "grid": a piano-roll style view of a pattern's steps.
//!
//! [`PatternGrid`] is the pure model (which pitches a pattern uses and at
//! which steps they sound); the `draw_*` functions turn it into pixels,
//! both as the full labeled grid of the properties panel and as the tiny
//! preview drawn inside an arrangement block.

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

/// A pattern reduced to what a grid needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternGrid {
    pub len_steps: usize,
    /// Distinct pitches used, highest first, labeled as written in the file
    /// (`C4`, `E4#`, ...). Enharmonic spellings share a row.
    pub rows: Vec<String>,
    pub notes: Vec<GridNote>,
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
        }
    }
}

pub const CELL_WIDTH: f32 = 20.0;
pub const CELL_HEIGHT: f32 = 18.0;
const LABEL_WIDTH: f32 = 40.0;
const HEADER_HEIGHT: f32 = 16.0;

/// Draws the full grid: pitch labels on the left, step numbers on top, beat
/// shading, and a filled cell for every sounding step.
pub fn draw_pattern_grid(ui: &mut egui::Ui, grid: &PatternGrid, steps_per_beat: usize, color: Color32) {
    let rows = grid.rows.len().max(1);
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

    // Row lines and pitch labels.
    for (i, label) in grid.rows.iter().enumerate() {
        let y = origin.y + i as f32 * CELL_HEIGHT;
        painter.line_segment([Pos2::new(origin.x, y), Pos2::new(rect.max.x, y)], line);
        painter.text(
            Pos2::new(origin.x - 6.0, y + CELL_HEIGHT / 2.0),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(11.0),
            text_color,
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
    for note in &grid.notes {
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

/// Draws a tiny preview of the pattern inside `rect` (an arrangement
/// block): one thin bar per sounding step, positioned by step and pitch row.
pub fn draw_mini_notes(painter: &egui::Painter, rect: Rect, grid: &PatternGrid, color: Color32) {
    if grid.len_steps == 0 || grid.notes.is_empty() {
        return;
    }
    let rows = grid.rows.len().max(1) as f32;
    let step_w = rect.width() / grid.len_steps as f32;
    let row_h = (rect.height() / rows).min(6.0);
    // Center the rows vertically when they don't fill the block.
    let top = rect.min.y + (rect.height() - row_h * rows) / 2.0;

    for note in &grid.notes {
        let min = Pos2::new(
            rect.min.x + note.step as f32 * step_w,
            top + note.row as f32 * row_h,
        );
        painter.rect_filled(
            Rect::from_min_size(min, Vec2::new((step_w - 1.0).max(1.0), (row_h - 1.0).max(1.0))),
            1.0,
            color,
        );
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
    fn grid_and_mini_preview_draw_without_panicking() {
        let g = PatternGrid::from_steps(&steps(&[Some("C4"), None, Some("E4"), None]));
        egui::__run_test_ui(|ui| {
            draw_pattern_grid(ui, &g, 4, Color32::LIGHT_BLUE);
            let painter = ui.painter().clone();
            draw_mini_notes(&painter, Rect::from_min_size(Pos2::ZERO, Vec2::new(80.0, 30.0)), &g, Color32::WHITE);
        });
    }
}
