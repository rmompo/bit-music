//! Area D: the arrangement. One row per track — its header on the left
//! (mute, a color swatch and the name, over a row of view-mode buttons,
//! always pinned to the top of the row) and a step grid on the right where
//! every pattern is a filled block (colored by its sample, with its notes
//! drawn inside, taller or shorter depending on the track's own view mode).
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
use crate::widgets::{paint_scope_multi, IconButton};
use crate::view::{Selection, TrackViewMode, ViewState};

const PLAYHEAD_COLOR: Color32 = Color32::from_rgb(255, 90, 90);
/// Pixels kept between the left edge of the grid and the cursor when the
/// view scrolls to follow it.
const FOLLOW_MARGIN: f32 = 60.0;

/// A pattern clip's opacity: always translucent, at one of three levels
/// depending on its track's state — the more "present" the state, the more
/// visible the card. An enabled track is the most visible, a muted one the
/// least, with a loop repetition in between.
const CLIP_OPACITY_ENABLED: f32 = 0.5;
const CLIP_OPACITY_LOOP: f32 = 0.3;
const CLIP_OPACITY_MUTED: f32 = 0.1;

/// Color of the live oscilloscope behind a track's name (translucent).
const SCOPE_COLOR: Color32 = Color32::from_rgba_premultiplied(45, 90, 115, 115);
/// Width of the pinned TRACK column, in points.
pub const LEFT_WIDTH: f32 = 176.0;
const RULER_HEIGHT: f32 = 24.0;
/// Height of each of the track header's two rows (mute/color/name, then the
/// view-mode buttons). The header is always pinned to the top of the row,
/// whatever the row's own height is; anything past it is left blank.
const HEADER_ROW_HEIGHT: f32 = 24.0;
const HEADER_HEIGHT: f32 = HEADER_ROW_HEIGHT * 2.0;
/// The smallest height of a track row, in points: never less than the
/// header needs.
const MIN_ROW_HEIGHT: f32 = HEADER_HEIGHT;
/// The tallest a track row may grow to.
const MAX_ROW_HEIGHT: f32 = 240.0;
const BLOCK_INSET: f32 = 4.0;
/// Space above the notes of a block, for its label.
const BLOCK_LABEL_HEIGHT: f32 = 15.0;
/// Space kept under the notes of a block.
const BLOCK_BOTTOM_PAD: f32 = 3.0;
/// Side of the small buttons in the header (mute, view mode).
const MODE_BUTTON_SIZE: f32 = 20.0;
/// Gap between the view-mode buttons.
const MODE_BUTTON_GAP: f32 = 3.0;
/// Side of the track's color indicator, next to its name.
const COLOR_SWATCH_SIZE: f32 = 12.0;

/// `color`, blended towards white by `amount` (`0.0` = unchanged, `1.0` =
/// white), alpha untouched. Used for the selected-pattern outline: the
/// pattern's own color, but brighter, instead of an unrelated accent color.
fn brighten(color: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let lerp = |c: u8| (c as f32 + (255.0 - c as f32) * amount).round() as u8;
    Color32::from_rgba_unmultiplied(lerp(color.r()), lerp(color.g()), lerp(color.b()), color.a())
}

/// How tall a track's rows are and how tall each of its note marks is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackMetrics {
    pub row_height: f32,
    /// Height of one semitone (one note mark) in every block.
    pub mark_height: f32,
}

/// Sizes one track's row so that every note mark in it has the same height:
/// as tall as its widest range of pitches (`max_rows` semitones) needs at
/// `target_mark_height` each (the track's view mode — see
/// [`crate::view::TrackViewMode::mark_height`]). Only if that would pass the
/// maximum are the marks made thinner, all of them the same.
pub fn track_metrics(max_rows: usize, target_mark_height: f32) -> TrackMetrics {
    let fixed = 2.0 * BLOCK_INSET + BLOCK_LABEL_HEIGHT + BLOCK_BOTTOM_PAD;
    let rows = max_rows.max(1) as f32;
    let row_height = (fixed + rows * target_mark_height).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
    let mark_height = ((row_height - fixed) / rows).clamp(1.0, target_mark_height);
    TrackMetrics { row_height, mark_height }
}

/// Draws the arrangement. `playhead` is the playback position in seconds
/// (`None` when there is nothing to play); while `playing`, the view scrolls
/// horizontally to keep the cursor visible.
/// `scopes` holds, per track, one live oscilloscope trace per channel,
/// drawn behind that track's name in the pinned column (empty when there is
/// nothing to show).
pub fn show(
    ui: &mut egui::Ui,
    l: &Loaded,
    view: &mut ViewState,
    playhead: Option<f64>,
    playing: bool,
    scopes: &[Vec<Vec<f32>>],
) {
    let timeline = &l.timeline;
    let spb = (l.project.composition.metadata.steps_per_beat as usize).max(1);
    let sw = view.step_width;
    // Each track is sized on its own: the widest range of pitches among
    // *its* patterns, at the mark height its own view mode asks for.
    let metrics: Vec<TrackMetrics> = timeline
        .tracks
        .iter()
        .enumerate()
        .map(|(ti, t)| {
            let max_rows = t
                .clips
                .iter()
                .filter_map(|c| view.grids.get(&c.pattern_id))
                .map(|g| g.chromatic.len())
                .max()
                .unwrap_or(0);
            let mode = view.track_view_modes.get(ti).copied().unwrap_or_default();
            track_metrics(max_rows, mode.mark_height())
        })
        .collect();
    // Cumulative offset of the top of each row, relative to `rows_top`;
    // `row_tops[tracks.len()]` is the total height of every row together.
    let mut row_tops = Vec::with_capacity(metrics.len() + 1);
    let mut acc = 0.0;
    row_tops.push(0.0);
    for m in &metrics {
        acc += m.row_height;
        row_tops.push(acc);
    }
    let total_rows_height = acc;
    let content = Vec2::new(
        LEFT_WIDTH + timeline.total_steps as f32 * sw,
        RULER_HEIGHT + total_rows_height,
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
            let rows_bottom = rows_top + total_rows_height;
            let grid_right = origin.x + width;

            // 1. Row backgrounds.
            for i in 0..timeline.tracks.len() {
                let y = rows_top + row_tops[i];
                let fill = if i % 2 == 0 { visuals.extreme_bg_color } else { visuals.faint_bg_color };
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(origin.x + LEFT_WIDTH, y), Pos2::new(grid_right, rows_top + row_tops[i + 1])),
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
                let y = rows_top + row_tops[i];
                painter.line_segment([Pos2::new(origin.x + LEFT_WIDTH, y), Pos2::new(grid_right, y)], strong_line);
            }

            // 5. Pattern blocks.
            for (ti, track) in timeline.tracks.iter().enumerate() {
                let muted = view.muted[ti];
                let y = rows_top + row_tops[ti];
                let TrackMetrics { row_height, mark_height } = metrics[ti];
                for clip in &track.clips {
                    let clip_end = clip.start_step + clip.len_steps;
                    if clip_end < step_lo || clip.start_step > step_hi {
                        continue;
                    }
                    let block = Rect::from_min_size(
                        Pos2::new(x_of(clip.start_step), y + BLOCK_INSET),
                        Vec2::new(clip.len_steps as f32 * sw, row_height - 2.0 * BLOCK_INSET),
                    );
                    let base = view.pattern_color(l, &clip.pattern_id);
                    let strength = match (muted, clip.is_repeat) {
                        (true, _) => CLIP_OPACITY_MUTED,
                        (false, true) => CLIP_OPACITY_LOOP,
                        (false, false) => CLIP_OPACITY_ENABLED,
                    };
                    painter.rect_filled(block, 0.0, base.gamma_multiply(strength));
                    painter.rect_stroke(block, 0.0, Stroke::new(1.0, base), StrokeKind::Inside);

                    if let Some(g) = view.grids.get(&clip.pattern_id) {
                        let notes_area = Rect::from_min_max(
                            Pos2::new(block.min.x + 3.0, block.min.y + BLOCK_LABEL_HEIGHT),
                            Pos2::new(block.max.x - 3.0, block.max.y - BLOCK_BOTTOM_PAD),
                        );
                        let notes_color = if muted {
                            Color32::from_white_alpha(70)
                        } else {
                            Color32::from_white_alpha(235)
                        };
                        grid::draw_mini_notes(
                            &painter.with_clip_rect(block.intersect(painter.clip_rect())),
                            notes_area,
                            g,
                            mark_height,
                            notes_color,
                        );
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
                        painter.rect_stroke(block, 0.0, Stroke::new(2.0, brighten(base, 0.6)), StrokeKind::Inside);
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
            // Each track's header is two rows — mute, color and name, then
            // the view-mode buttons — always pinned to the top of the cell,
            // whatever its height: any extra room stays blank below them.
            // The oscilloscope is painted faintly behind the whole cell.
            let x_pin = origin.x + viewport.min.x;
            for (ti, track) in timeline.tracks.iter().enumerate() {
                let y = rows_top + row_tops[ti];
                let row_height = metrics[ti].row_height;
                let cell = Rect::from_min_size(Pos2::new(x_pin, y), Vec2::new(LEFT_WIDTH, row_height));
                painter.rect_filled(cell, 0.0, visuals.panel_fill);
                if let Some(scope) = scopes.get(ti) {
                    // Faint, behind the header. One band per channel.
                    paint_scope_multi(&painter, cell, scope, SCOPE_COLOR);
                }
                painter.line_segment([cell.left_bottom(), cell.right_bottom()], strong_line);

                // The header's own content — text, color swatch, button
                // icons — fades while playing, like the sample/pattern rows
                // in the properties panel, so the oscilloscope behind it
                // reads clearly; full strength otherwise. `Ui::set_opacity`
                // multiplies the alpha of *everything* painted through this
                // scope afterwards, widgets included, whatever color each
                // one resolves to on its own — simpler and more reliable
                // than fading each color by hand.
                let muted = view.muted[ti];
                let content_alpha = (if muted { 0.5 } else { 1.0 }) * (if playing { 0.35 } else { 1.0 });

                ui.scope(|ui| {
                    ui.set_opacity(content_alpha);

                    // Row 1: mute, the track's color, then its name.
                    let row1_mid_y = cell.min.y + HEADER_ROW_HEIGHT / 2.0;
                    let mute_button = Rect::from_min_size(
                        Pos2::new(cell.min.x + 8.0, row1_mid_y - MODE_BUTTON_SIZE / 2.0),
                        Vec2::splat(MODE_BUTTON_SIZE),
                    );
                    let icon = if muted { regular::SPEAKER_SLASH } else { regular::SPEAKER_HIGH };
                    let response = ui
                        .put(mute_button, IconButton::new(icon).selected(muted).size(MODE_BUTTON_SIZE))
                        .on_hover_text(if muted { t("tracks.unmute") } else { t("tracks.mute") });
                    if response.clicked() {
                        view.muted[ti] = !muted;
                    }

                    let swatch = Rect::from_min_size(
                        Pos2::new(mute_button.max.x + 6.0, row1_mid_y - COLOR_SWATCH_SIZE / 2.0),
                        Vec2::splat(COLOR_SWATCH_SIZE),
                    );
                    // The color of the track's first pattern's sample: a
                    // fixed, representative color even when later clips use
                    // others.
                    let track_color = track
                        .clips
                        .first()
                        .map(|c| view.pattern_color(l, &c.pattern_id))
                        .unwrap_or(Color32::GRAY);
                    ui.painter().rect_filled(swatch, 2.0, track_color);

                    ui.painter().with_clip_rect(cell).text(
                        Pos2::new(swatch.max.x + 6.0, row1_mid_y),
                        Align2::LEFT_CENTER,
                        &track.id,
                        FontId::proportional(14.0),
                        text_color,
                    );

                    // Row 2: the view-mode buttons.
                    let row2_mid_y = cell.min.y + HEADER_ROW_HEIGHT + HEADER_ROW_HEIGHT / 2.0;
                    let mut bx = cell.min.x + 8.0;
                    for mode in TrackViewMode::ALL {
                        let button = Rect::from_min_size(
                            Pos2::new(bx, row2_mid_y - MODE_BUTTON_SIZE / 2.0),
                            Vec2::splat(MODE_BUTTON_SIZE),
                        );
                        let selected = view.track_view_modes[ti] == mode;
                        let tooltip = match mode {
                            TrackViewMode::Compact => t("tracks.view_compact"),
                            TrackViewMode::Standard => t("tracks.view_standard"),
                            TrackViewMode::Full => t("tracks.view_full"),
                        };
                        let response = ui
                            .put(button, IconButton::new(mode.icon()).selected(selected).size(MODE_BUTTON_SIZE))
                            .on_hover_text(tooltip);
                        if response.clicked() {
                            view.track_view_modes[ti] = mode;
                        }
                        bx += MODE_BUTTON_SIZE + MODE_BUTTON_GAP;
                    }
                });
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
    fn brighten_moves_toward_white_and_keeps_alpha() {
        let color = Color32::from_rgb(80, 190, 190);
        assert_eq!(brighten(color, 0.0), color);
        assert_eq!(brighten(color, 1.0), Color32::from_rgba_unmultiplied(255, 255, 255, 255));
        let half = brighten(color, 0.5);
        assert!(half.r() > color.r() && half.r() < 255);
        assert!(half.g() > color.g() && half.g() < 255);
        assert!(half.b() > color.b() && half.b() < 255);
        assert_eq!(half.a(), color.a());
        // Clamped, not panicking, on out-of-range input.
        assert_eq!(brighten(color, 2.0), brighten(color, 1.0));
        assert_eq!(brighten(color, -1.0), color);
    }

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
            // Two channels per track, to exercise the multi-channel path too.
            let scopes = vec![vec![vec![0.0, 0.8, -0.8, 0.3], vec![0.0, -0.5, 0.5, -0.2]]; l.timeline.tracks.len()];
            egui::__run_test_ui(|ui| show(ui, &l, &mut view, Some(t), true, &scopes));
        }
    }

    #[test]
    fn the_tracks_are_as_tall_as_the_widest_range_needs_and_every_mark_is_alike() {
        let standard = TrackViewMode::Standard.mark_height();
        // No notes: the minimum height, marks at their normal height.
        let none = track_metrics(0, standard);
        assert_eq!(none.row_height, MIN_ROW_HEIGHT);
        assert_eq!(none.mark_height, standard);
        // One octave: 26 points of frame and label + 12 semitones of 4 points.
        let one = track_metrics(12, standard);
        assert_eq!(one.row_height, 74.0);
        assert_eq!(one.mark_height, standard);
        // Two octaves: taller rows, the same mark height.
        let two = track_metrics(24, standard);
        assert_eq!(two.row_height, 122.0);
        assert_eq!(two.mark_height, standard);
        // A huge range hits the maximum row height: the marks get thinner,
        // but they still all have the same height, and they fit.
        let huge = track_metrics(120, standard);
        assert_eq!(huge.row_height, MAX_ROW_HEIGHT);
        assert!(huge.mark_height < standard && huge.mark_height >= 1.0);
        assert!(120.0 * huge.mark_height <= MAX_ROW_HEIGHT - 26.0 + 1e-3);
    }

    #[test]
    fn a_wider_target_mark_height_gives_a_taller_row_for_the_same_range() {
        let compact = track_metrics(12, TrackViewMode::Compact.mark_height());
        let standard = track_metrics(12, TrackViewMode::Standard.mark_height());
        let full = track_metrics(12, TrackViewMode::Full.mark_height());
        assert!(compact.row_height < standard.row_height);
        assert!(standard.row_height < full.row_height);
    }

    fn demo() -> Box<Loaded> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1");
        match load_blocking(&path) {
            LoadOutcome::Loaded(l) => l,
            LoadOutcome::Failed { message, .. } => panic!("demo should load: {message}"),
        }
    }

    #[test]
    fn each_track_is_sized_independently_from_its_own_view_mode() {
        let l = demo();
        let mut view = ViewState::new(&l);
        assert!(l.timeline.tracks.len() >= 2, "the demo needs at least two tracks for this test");
        view.track_view_modes[0] = TrackViewMode::Full;
        view.track_view_modes[1] = TrackViewMode::Compact;
        egui::__run_test_ui(|ui| show(ui, &l, &mut view, None, false, &[]));

        // Recomputed the same way `show` does, from each track's own clips.
        let heights: Vec<f32> = l
            .timeline
            .tracks
            .iter()
            .enumerate()
            .map(|(ti, t)| {
                let max_rows = t
                    .clips
                    .iter()
                    .filter_map(|c| view.grids.get(&c.pattern_id))
                    .map(|g| g.chromatic.len())
                    .max()
                    .unwrap_or(0);
                track_metrics(max_rows, view.track_view_modes[ti].mark_height()).row_height
            })
            .collect();
        assert!(heights[0] > heights[1], "a Full track should be taller than a Compact one: {heights:?}");
    }

    #[test]
    fn draws_with_a_mix_of_view_modes_without_panicking() {
        let l = demo();
        let mut view = ViewState::new(&l);
        for (ti, mode) in TrackViewMode::ALL.iter().cycle().take(l.timeline.tracks.len()).enumerate() {
            view.track_view_modes[ti] = *mode;
        }
        egui::__run_test_ui(|ui| show(ui, &l, &mut view, None, false, &[]));
    }
}
