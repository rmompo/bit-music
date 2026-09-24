//! Area D: the arrangement. One row per track — a mute button and the track
//! name on the left, a step grid on the right where every pattern is a
//! filled block (colored by its sample, with its notes drawn inside).
//!
//! Everything lives in a single 2D scroll area, so all tracks scroll
//! together. The left column and the ruler are "pinned" by painting them at
//! the viewport's current offset, so they stay put while the rest scrolls.

use egui_phosphor::regular;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::fmt;
use crate::i18n::{t, tf};
use crate::grid;
use crate::loader::Loaded;
use crate::widgets::{paint_scope, IconButton, ICON_BUTTON_SIZE};
use crate::view::{Selection, ViewState};

const PLAYHEAD_COLOR: Color32 = Color32::from_rgb(255, 90, 90);
/// Pixels kept between the left edge of the grid and the cursor when the
/// view scrolls to follow it.
const FOLLOW_MARGIN: f32 = 60.0;

/// Color of the live oscilloscope behind a track's name (translucent).
const SCOPE_COLOR: Color32 = Color32::from_rgba_premultiplied(45, 90, 115, 115);
/// Width of the pinned TRACK column, in points.
pub const LEFT_WIDTH: f32 = 176.0;
const RULER_HEIGHT: f32 = 24.0;
const ROW_HEIGHT: f32 = 52.0;
const BLOCK_INSET: f32 = 4.0;

/// Draws the arrangement. `playhead` is the playback position in seconds
/// (`None` when there is nothing to play); while `playing`, the view scrolls
/// horizontally to keep the cursor visible.
/// `scopes` holds, per track, the live oscilloscope values drawn behind that
/// track's name in the pinned column (empty when there is nothing to show).
pub fn show(
    ui: &mut egui::Ui,
    l: &Loaded,
    view: &mut ViewState,
    playhead: Option<f64>,
    playing: bool,
    scopes: &[Vec<f32>],
) {
    let timeline = &l.timeline;
    let spb = (l.project.composition.metadata.steps_per_beat as usize).max(1);
    let sw = view.step_width;
    let content = Vec2::new(
        LEFT_WIDTH + timeline.total_steps as f32 * sw,
        RULER_HEIGHT + timeline.tracks.len() as f32 * ROW_HEIGHT,
    );

    let mut clicked: Option<String> = None;

    let mut area = egui::ScrollArea::both()
        .id_salt("arrangement_scroll")
        .auto_shrink([false, false]);
    if let Some(offset) = view.scroll_to.take() {
        area = area.horizontal_scroll_offset(offset);
    }
    let mut follow_to: Option<f32> = None;

    area.show_viewport(ui, |ui, viewport| {
            // Rows fill the whole viewport even when the song is shorter.
            let width = content.x.max(viewport.width());
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, content.y), Sense::hover());
            let origin = rect.min;
            let painter = ui.painter().clone();
            let visuals = ui.visuals().clone();
            let text_color = visuals.text_color();
            let weak_line = Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.5));
            let strong_line = Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);

            // Visible step range (culling).
            let step_lo = (((viewport.min.x - LEFT_WIDTH) / sw).floor().max(0.0)) as usize;
            let step_hi = (((viewport.max.x - LEFT_WIDTH) / sw).ceil().max(0.0) as usize)
                .min(timeline.total_steps);
            let x_of = |step: usize| origin.x + LEFT_WIDTH + step as f32 * sw;
            let rows_top = origin.y + RULER_HEIGHT;
            let rows_bottom = rows_top + timeline.tracks.len() as f32 * ROW_HEIGHT;
            let grid_right = origin.x + width;

            // 1. Row backgrounds.
            for i in 0..timeline.tracks.len() {
                let y = rows_top + i as f32 * ROW_HEIGHT;
                let fill = if i % 2 == 0 { visuals.extreme_bg_color } else { visuals.faint_bg_color };
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(origin.x + LEFT_WIDTH, y), Pos2::new(grid_right, y + ROW_HEIGHT)),
                    0.0,
                    fill,
                );
            }

            // 2. Column shading (every other column) and column boundaries.
            for (ci, column) in timeline.columns.iter().enumerate() {
                let end = column.start_step + column.len_steps;
                if end < step_lo || column.start_step > step_hi {
                    continue;
                }
                let (x0, x1) = (x_of(column.start_step), x_of(end));
                if ci % 2 == 1 {
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(x0, rows_top), Pos2::new(x1, rows_bottom)),
                        0.0,
                        Color32::from_black_alpha(28),
                    );
                }
                painter.line_segment([Pos2::new(x0, rows_top), Pos2::new(x0, rows_bottom)], strong_line);
            }

            // 3. Beat lines.
            for step in (step_lo..=step_hi).filter(|s| s % spb == 0) {
                let x = x_of(step);
                painter.line_segment([Pos2::new(x, rows_top), Pos2::new(x, rows_bottom)], weak_line);
            }

            // 4. Row separators.
            for i in 0..=timeline.tracks.len() {
                let y = rows_top + i as f32 * ROW_HEIGHT;
                painter.line_segment([Pos2::new(origin.x + LEFT_WIDTH, y), Pos2::new(grid_right, y)], strong_line);
            }

            // 5. Pattern blocks.
            for (ti, track) in timeline.tracks.iter().enumerate() {
                let muted = view.muted[ti];
                let y = rows_top + ti as f32 * ROW_HEIGHT;
                for clip in &track.clips {
                    let clip_end = clip.start_step + clip.len_steps;
                    if clip_end < step_lo || clip.start_step > step_hi {
                        continue;
                    }
                    let block = Rect::from_min_size(
                        Pos2::new(x_of(clip.start_step), y + BLOCK_INSET),
                        Vec2::new(clip.len_steps as f32 * sw, ROW_HEIGHT - 2.0 * BLOCK_INSET),
                    );
                    let base = view.pattern_color(l, &clip.pattern_id);
                    let strength = match (muted, clip.is_repeat) {
                        (true, _) => 0.25,
                        (false, true) => 0.55,
                        (false, false) => 0.9,
                    };
                    painter.rect_filled(block, 4.0, base.gamma_multiply(strength));
                    painter.rect_stroke(block, 4.0, Stroke::new(1.0, base), StrokeKind::Inside);

                    if let Some(g) = view.grids.get(&clip.pattern_id) {
                        let notes_area = Rect::from_min_max(
                            Pos2::new(block.min.x + 3.0, block.min.y + 15.0),
                            Pos2::new(block.max.x - 3.0, block.max.y - 3.0),
                        );
                        let notes_color = if muted {
                            Color32::from_white_alpha(70)
                        } else {
                            Color32::from_white_alpha(235)
                        };
                        grid::draw_mini_notes(&painter.with_clip_rect(block.intersect(painter.clip_rect())), notes_area, g, notes_color);
                    }

                    let label = if clip.is_repeat {
                        tf("clip.loop", &[("pattern", &clip.pattern_id)])
                    } else {
                        clip.pattern_id.clone()
                    };
                    painter
                        .with_clip_rect(block.intersect(painter.clip_rect()))
                        .text(block.min + Vec2::new(4.0, 2.0), Align2::LEFT_TOP, label, FontId::proportional(11.0), Color32::WHITE);

                    if view.selected_pattern.as_deref() == Some(clip.pattern_id.as_str()) {
                        painter.rect_stroke(block, 4.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Outside);
                    }

                    let response = ui.interact(block, ui.id().with(("clip", ti, clip.column)), Sense::click());
                    if response.clicked() {
                        clicked = Some(clip.pattern_id.clone());
                    }
                    response.on_hover_text(tf(
                        "clip.hover",
                        &[
                            ("pattern", &clip.pattern_id),
                            ("steps", &clip.len_steps.to_string()),
                            ("repeat", if clip.is_repeat { t("clip.repeated") } else { "" }),
                        ],
                    ));
                }
            }

            // 5b. Playback cursor.
            let playhead_x = playhead
                .map(|t| t / l.seconds_per_step)
                .filter(|step| *step <= timeline.total_steps as f64)
                .map(|step| origin.x + LEFT_WIDTH + step as f32 * sw);
            if let Some(x) = playhead_x {
                painter.line_segment(
                    [Pos2::new(x, rows_top), Pos2::new(x, rows_bottom)],
                    Stroke::new(2.0, PLAYHEAD_COLOR),
                );
                // Keep the cursor in view while playing.
                let left = origin.x + viewport.min.x + LEFT_WIDTH;
                let right = origin.x + viewport.max.x - FOLLOW_MARGIN;
                if playing && (x < left || x > right) {
                    follow_to = Some(((x - origin.x - LEFT_WIDTH) - FOLLOW_MARGIN).max(0.0));
                }
            }

            // 6. Pinned left column (x follows the horizontal scroll offset).
            let x_pin = origin.x + viewport.min.x;
            for (ti, track) in timeline.tracks.iter().enumerate() {
                let y = rows_top + ti as f32 * ROW_HEIGHT;
                let cell = Rect::from_min_size(Pos2::new(x_pin, y), Vec2::new(LEFT_WIDTH, ROW_HEIGHT));
                painter.rect_filled(cell, 0.0, visuals.panel_fill);
                if let Some(scope) = scopes.get(ti) {
                    // Faint, behind the button and the name.
                    paint_scope(&painter, cell, scope, SCOPE_COLOR);
                }
                painter.line_segment([cell.left_bottom(), cell.right_bottom()], strong_line);

                let button = Rect::from_min_size(
                    Pos2::new(cell.min.x + 8.0, y + (ROW_HEIGHT - ICON_BUTTON_SIZE) / 2.0),
                    Vec2::splat(ICON_BUTTON_SIZE),
                );
                let muted = view.muted[ti];
                let icon = if muted { regular::SPEAKER_SLASH } else { regular::SPEAKER_HIGH };
                let response = ui
                    .put(button, IconButton::new(icon).selected(muted))
                    .on_hover_text(if muted { t("tracks.unmute") } else { t("tracks.mute") });
                if response.clicked() {
                    view.muted[ti] = !muted;
                }

                let name_color = if view.muted[ti] { text_color.gamma_multiply(0.5) } else { text_color };
                painter
                    .with_clip_rect(cell.shrink2(Vec2::new(0.0, 0.0)))
                    .text(
                        Pos2::new(button.max.x + 8.0, y + ROW_HEIGHT / 2.0),
                        Align2::LEFT_CENTER,
                        &track.id,
                        FontId::proportional(14.0),
                        name_color,
                    );
            }
            painter.line_segment(
                [Pos2::new(x_pin + LEFT_WIDTH, rows_top.max(origin.y + viewport.min.y)), Pos2::new(x_pin + LEFT_WIDTH, rows_bottom)],
                strong_line,
            );

            // 7. Pinned ruler (y follows the vertical scroll offset).
            let y_pin = origin.y + viewport.min.y;
            let ruler = Rect::from_min_size(Pos2::new(x_pin, y_pin), Vec2::new(viewport.width(), RULER_HEIGHT));
            painter.rect_filled(ruler, 0.0, visuals.panel_fill);
            painter.line_segment([ruler.left_bottom(), ruler.right_bottom()], strong_line);
            let ruler_painter = painter.with_clip_rect(ruler.intersect(painter.clip_rect()));
            for step in (step_lo..=step_hi).filter(|s| s % spb == 0) {
                let x = x_of(step);
                ruler_painter.line_segment([Pos2::new(x, y_pin + RULER_HEIGHT - 5.0), Pos2::new(x, y_pin + RULER_HEIGHT)], strong_line);
            }
            for (ci, column) in timeline.columns.iter().enumerate() {
                let x = x_of(column.start_step);
                let wide = column.len_steps as f32 * sw >= 84.0;
                let text = if wide {
                    format!("{}  {}", ci + 1, fmt::duration_mmss(column.start_step as f64 * l.seconds_per_step))
                } else {
                    (ci + 1).to_string()
                };
                ruler_painter.line_segment([Pos2::new(x, y_pin), Pos2::new(x, y_pin + RULER_HEIGHT)], strong_line);
                ruler_painter.text(
                    Pos2::new(x + 4.0, y_pin + 4.0),
                    Align2::LEFT_TOP,
                    text,
                    FontId::proportional(11.0),
                    text_color,
                );
            }

            if let Some(x) = playhead_x {
                ruler_painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(x - 5.0, y_pin + RULER_HEIGHT - 9.0),
                        Pos2::new(x + 5.0, y_pin + RULER_HEIGHT - 9.0),
                        Pos2::new(x, y_pin + RULER_HEIGHT),
                    ],
                    PLAYHEAD_COLOR,
                    Stroke::NONE,
                ));
            }

            // 8. Pinned corner.
            let corner = Rect::from_min_size(Pos2::new(x_pin, y_pin), Vec2::new(LEFT_WIDTH, RULER_HEIGHT));
            painter.rect_filled(corner, 0.0, visuals.panel_fill);
            painter.line_segment([corner.left_bottom(), corner.right_bottom()], strong_line);
            painter.text(
                corner.left_center() + Vec2::new(8.0, 0.0),
                Align2::LEFT_CENTER,
                t("tracks.corner"),
                FontId::proportional(11.0),
                text_color.gamma_multiply(0.7),
            );
        });

    if follow_to.is_some() {
        view.scroll_to = follow_to;
    }
    if let Some(id) = clicked {
        view.select(Selection::Pattern(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{load_blocking, LoadOutcome};
    use std::path::Path;

    #[test]
    fn draws_the_demo_arrangement_without_panicking_at_several_zooms() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1");
        let LoadOutcome::Loaded(l) = load_blocking(&path) else {
            panic!("demo should load");
        };
        let mut view = ViewState::new(&l);
        for zoom in [6.0, 16.0, 48.0] {
            view.step_width = zoom;
            egui::__run_test_ui(|ui| show(ui, &l, &mut view, None, false, &[]));
        }
        view.muted[0] = true;
        view.select(Selection::Pattern("kickA".into()));
        // with a playback cursor inside, at the end, and past the end
        for t in [0.0, 1.3, 2.5, 6.0] {
            let scopes = vec![vec![0.0, 0.8, -0.8, 0.3]; l.timeline.tracks.len()];
            egui::__run_test_ui(|ui| show(ui, &l, &mut view, Some(t), true, &scopes));
        }
    }
}
