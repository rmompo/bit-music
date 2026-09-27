//! The layout of the window once a composition is open:
//!
//! ```text
//! +--------------------------------+
//! | menu bar (in app.rs)           |
//! +---------+----------------------+
//! |    A    |                      |   A: tabs (Metadata, Samples, Patterns)
//! +----x----y          C           |   B: properties (one column)
//! |    B    |                      |   C: the tracks
//! |         +----------------------+   transport: under C only
//! |         | transport            |
//! +---------+----------------------+
//! | footer (in app.rs)             |
//! +--------------------------------+
//! ```
//!
//! `y` is the vertical divider (between the left column and C) and `x` the
//! horizontal one (between A and B); both are dragged with the mouse. Their
//! positions live in the view as percentages and are always what sizes the
//! panels, so they are applied on every start and follow the window when it
//! is resized. Each is limited by two floors at once: no less than a
//! percentage and no less than a number of points; the points are never
//! broken (see [`clamp_percent`]).

use eframe::egui::{self, Sense, Vec2};

use crate::arrangement;
use crate::config::DividerLimit;
use crate::loader::Loaded;
use crate::panels;
use crate::transport::{self, Transport};
use crate::view::ViewState;

/// Keeps a divider position (a percentage of `total` points) within its
/// limits, obeying two floors at once: no less than `limit.min` percent and no
/// less than `limit.min_points` points, on each side. The points are never
/// broken: they win over the percentages, and if `total` cannot hold both
/// sides at the floor, the room is split evenly.
pub fn clamp_percent(percent: f32, total: f32, limit: DividerLimit) -> f32 {
    let floor = limit.min_points / total.max(1.0) * 100.0;
    if floor * 2.0 >= 100.0 {
        return 50.0;
    }
    // The points on each side are the hard bounds; the percentages narrow
    // them when they can.
    let (hard_lo, hard_hi) = (floor, 100.0 - floor);
    let lo = limit.min.max(hard_lo).min(hard_hi);
    let hi = limit.max.min(hard_hi).max(lo);
    percent.clamp(lo, hi)
}

/// Inner margin of the panels, in points: the default (about 8) wastes space.
const PANEL_MARGIN: i8 = 2;

/// The panel frame in the current style with the compact margin: every panel
/// in the window uses this, so they all carry the same margin (the menu bar
/// and status bar in `app.rs` included). The exception is a purely
/// structural container that only arranges other, already margined, panels
/// (the left column, the right side): that one must stay at [`no_margin`],
/// or an edge shared with one of its children would be inset twice — once
/// by each frame.
pub(crate) fn compact(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::same(PANEL_MARGIN))
}

/// The panel frame with no inner margin, for a container that only arranges
/// other (already margined) panels and should not add its own margin.
fn no_margin(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::ZERO)
}

/// Direction of a divider line.
#[derive(Clone, Copy)]
pub enum Axis {
    /// A vertical line, dragged left / right.
    Vertical,
    /// A horizontal line, dragged up / down.
    Horizontal,
}

/// A draggable divider line starting at `start` and `length` long. Returns
/// how far it was dragged this frame (in points, along its normal).
pub fn splitter(ui: &mut egui::Ui, id: &str, start: egui::Pos2, length: f32, axis: Axis) -> f32 {
    const GRAB: f32 = 6.0;
    let (rect, line) = match axis {
        Axis::Vertical => (
            egui::Rect::from_min_size(start - Vec2::new(GRAB / 2.0, 0.0), Vec2::new(GRAB, length)),
            [start, start + Vec2::new(0.0, length)],
        ),
        Axis::Horizontal => (
            egui::Rect::from_min_size(start - Vec2::new(0.0, GRAB / 2.0), Vec2::new(length, GRAB)),
            [start, start + Vec2::new(length, 0.0)],
        ),
    };
    let response = ui.interact(rect, ui.id().with(id), Sense::drag());
    let cursor = match axis {
        Axis::Vertical => egui::CursorIcon::ResizeHorizontal,
        Axis::Horizontal => egui::CursorIcon::ResizeVertical,
    };
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(cursor);
    }
    let visuals = ui.visuals();
    let stroke = if response.dragged() {
        visuals.widgets.active.fg_stroke
    } else if response.hovered() {
        visuals.widgets.hovered.fg_stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    ui.painter().line_segment(line, stroke);
    match axis {
        Axis::Vertical => response.drag_delta().x,
        Axis::Horizontal => response.drag_delta().y,
    }
}

/// Draws everything between the menu bar and the footer: the left column
/// (tabs over properties) and, at its right, the tracks with the transport
/// under them.
pub fn show(
    ui: &mut egui::Ui,
    l: &Loaded,
    view: &mut ViewState,
    transport: &Transport,
    scopes: &[Vec<f32>],
) {
    let total = ui.available_width();
    let limit = view.divider_limits.tracks_width;
    // The size comes from the stored percentage (kept within its limits even
    // if the window has just been resized).
    view.tracks_width_percent = clamp_percent(view.tracks_width_percent, total, limit);
    let left_width = total * (100.0 - view.tracks_width_percent) / 100.0;

    let left = egui::Panel::left("left_column")
        .resizable(false)
        .exact_size(left_width)
        .frame(no_margin(ui))
        .show(ui, |ui| left_column(ui, l, view, transport));

    // The vertical divider: dragging it right makes the left column wider,
    // so C narrower.
    let edge = left.response.rect;
    let delta = splitter(ui, "columns_splitter", edge.right_top(), edge.height(), Axis::Vertical);
    if total > 0.0 {
        view.tracks_width_percent =
            clamp_percent(view.tracks_width_percent - delta / total * 100.0, total, limit);
    }

    egui::CentralPanel::default().frame(no_margin(ui)).show(ui, |ui| {
        egui::Panel::bottom("transport_bar").frame(compact(ui)).show(ui, |ui| transport::show(ui, transport, view));
        compact(ui).show(ui, |ui| {
            arrangement::show(ui, l, view, transport.playhead(), transport.is_playing(), scopes);
        });
    });
}

/// The left column: A (the tabs) over B (the properties), with the
/// horizontal divider between them.
fn left_column(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState, transport: &Transport) {
    let total = ui.available_height();
    let limit = view.divider_limits.tabs_height;
    view.tabs_height_percent = clamp_percent(view.tabs_height_percent, total, limit);

    let tabs = egui::Panel::top("tabs_panel")
        .resizable(false)
        .exact_size(total * view.tabs_height_percent / 100.0)
        .frame(compact(ui))
        .show(ui, |ui| panels::lists(ui, l, view, transport));

    // Dragging it down makes A taller, so B shorter.
    let edge = tabs.response.rect;
    let delta = splitter(ui, "rows_splitter", edge.left_bottom(), edge.width(), Axis::Horizontal);
    if total > 0.0 {
        view.tabs_height_percent =
            clamp_percent(view.tabs_height_percent + delta / total * 100.0, total, limit);
    }

    egui::CentralPanel::default().frame(compact(ui)).show(ui, |ui| panels::properties(ui, l, view));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{load_blocking, LoadOutcome};
    use std::path::Path;

    /// C's share of the width: 50% to 75%, and never less than 120 points.
    const WIDTH: DividerLimit = DividerLimit { min: 50.0, max: 75.0, min_points: 120.0 };
    /// A's share of the height: 25% to 50%, and never less than 120 points.
    const HEIGHT: DividerLimit = DividerLimit { min: 25.0, max: 50.0, min_points: 120.0 };

    #[test]
    fn the_percentages_limit_the_divider_when_there_is_room() {
        // 1000 points: 120 points are 12%, which changes nothing here.
        assert_eq!(clamp_percent(10.0, 1000.0, WIDTH), 50.0);
        assert_eq!(clamp_percent(60.0, 1000.0, WIDTH), 60.0);
        assert_eq!(clamp_percent(95.0, 1000.0, WIDTH), 75.0);
        assert_eq!(clamp_percent(5.0, 1000.0, HEIGHT), 25.0);
        assert_eq!(clamp_percent(90.0, 1000.0, HEIGHT), 50.0);
    }

    #[test]
    fn the_points_floor_beats_the_percentage_floor_in_a_small_window() {
        // A left column of 370 points: 25% would be 92, under 120 points.
        let a = clamp_percent(10.0, 370.0, HEIGHT);
        assert!(a * 370.0 / 100.0 >= 120.0 - 1e-3, "A has {} points", a * 3.7);
        // The other side (B) never has less than 120 either, however far it is dragged.
        let b = clamp_percent(100.0, 370.0, HEIGHT);
        assert!((100.0 - b) * 370.0 / 100.0 >= 120.0 - 1e-3, "B has {} points", (100.0 - b) * 3.7);
    }

    #[test]
    fn the_points_floor_holds_for_the_side_that_the_percentages_would_squeeze() {
        // 300 points wide: C at 75% would leave 75 points for A + B.
        let c = clamp_percent(75.0, 300.0, WIDTH);
        assert!((100.0 - c) * 300.0 / 100.0 >= 120.0 - 1e-3);
        // Even a floor above the maximum percentage: the points win.
        let strict = DividerLimit { min: 60.0, max: 75.0, min_points: 120.0 };
        let p = clamp_percent(60.0, 280.0, strict);
        assert!((100.0 - p) * 280.0 / 100.0 >= 120.0 - 1e-3, "{p}");
    }

    #[test]
    fn a_room_too_small_for_both_floors_is_split_evenly() {
        assert_eq!(clamp_percent(75.0, 200.0, WIDTH), 50.0);
        assert_eq!(clamp_percent(30.0, 240.0, HEIGHT), 50.0);
    }

    fn demo() -> Box<Loaded> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1");
        match load_blocking(&path) {
            LoadOutcome::Loaded(l) => l,
            LoadOutcome::Failed { message, .. } => panic!("demo failed to load: {message}"),
        }
    }

    #[test]
    fn the_whole_layout_draws_at_several_window_sizes() {
        let l = demo();
        let t = Transport::new(&l);
        let mut view = ViewState::new(&l);
        view.select(crate::view::Selection::Pattern("saxA".into()));
        // The default positions, then extreme ones the clamping must repair.
        for (width, height) in [(75.0, 25.0), (0.0, 0.0), (100.0, 100.0), (60.0, 40.0)] {
            view.tracks_width_percent = width;
            view.tabs_height_percent = height;
            egui::__run_test_ui(|ui| show(ui, &l, &mut view, &t, &[]));
            let (w, h) = (view.tracks_width_percent, view.tabs_height_percent);
            assert!((50.0..=75.0).contains(&w), "C width {w}");
            assert!((25.0..=50.0).contains(&h) || h == 50.0, "A height {h}");
        }
    }
}
