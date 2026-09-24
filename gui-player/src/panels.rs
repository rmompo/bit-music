//! The top row of the window: B (tabs Metadata / Samples / Patterns) and C
//! (properties of whatever is selected in B or in the arrangement).

use eframe::egui::{self, Color32, RichText, Sense, Vec2};
use egui_phosphor::regular;

use crate::fmt;
use crate::grid;
use crate::loader::Loaded;
use crate::transport::Transport;
use crate::widgets::{paint_scope, scope_points, IconButton, ICON_BUTTON_SIZE};
use crate::view::{ListTab, ViewState};

pub const OK_COLOR: Color32 = Color32::from_rgb(90, 190, 110);
pub const ERR_COLOR: Color32 = Color32::from_rgb(230, 90, 90);

/// Minimum width of the key column in property tables, so tables stacked
/// in one panel line their values up.
pub const MIN_KEY_WIDTH: f32 = 130.0;

/// Smallest size of either side of a divider, in points.
const MIN_PANEL_SIZE: f32 = 120.0;
/// Keeps a divider position within `(min, max)` (the limits of its schema
/// entry) and leaves at least [`MIN_PANEL_SIZE`] points on each side of
/// `total`.
pub fn clamp_percent(percent: f32, total: f32, (min, max): (f32, f32)) -> f32 {
    let min_by_size = (MIN_PANEL_SIZE / total.max(1.0) * 100.0).min(50.0);
    let lo = min.max(min_by_size);
    let hi = max.min(100.0 - min_by_size);
    percent.clamp(lo, hi.max(lo))
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

/// A | B: the tabs and the properties, each with its own vertical scroll,
/// with a draggable separator between them. Its position is
/// `view.tabs_width_percent` (30% by default) and follows the user's drag.
pub fn top_row(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState, transport: &Transport) {
    let total = ui.available_width();
    // The size always comes from the stored percentage, so it is applied on
    // every start and follows the window when it is resized.
    let tabs = egui::Panel::left("tabs_panel")
        .resizable(false)
        .exact_size(total * view.tabs_width_percent / 100.0)
        .show(ui, |ui| lists(ui, l, view, transport));
    let edge = tabs.response.rect;
    let delta = splitter(ui, "tabs_splitter", edge.right_top(), edge.height(), Axis::Vertical);
    if total > 0.0 {
        let range = view.divider_limits.tabs_width_range();
        view.tabs_width_percent = clamp_percent(view.tabs_width_percent + delta / total * 100.0, total, range);
    }
    egui::CentralPanel::default().show(ui, |ui| {
        properties(ui, l, view);
    });
}

fn scroll(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, add);
}

fn row(ui: &mut egui::Ui, key: &str, value: impl Into<String>) {
    ui.label(RichText::new(key).weak());
    ui.label(value.into());
    ui.end_row();
}

fn chip(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}

/// The Metadata tab: composition metadata (its `others` go to Properties).
pub fn metadata(ui: &mut egui::Ui, l: &Loaded) {
    let m = &l.project.composition.metadata;
    egui::Grid::new("metadata_grid")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            row(ui, "Title", m.title.clone());
            row(ui, "Format version", m.version.clone());
            row(ui, "BPM", m.bpm.to_string());
            row(ui, "Steps per beat", m.steps_per_beat.to_string());
            row(ui, "Seconds per step", format!("{:.3}", l.seconds_per_step));
            row(
                ui,
                "Length",
                format!(
                    "{} steps · {}",
                    l.timeline.total_steps,
                    fmt::duration_mmss(l.duration_seconds())
                ),
            );
        });
}

/// Properties while the Metadata tab is open: the `others` key/value pairs.
fn others_properties(ui: &mut egui::Ui, l: &Loaded) {
    let others = &l.project.composition.metadata.others;
    if others.is_empty() {
        ui.label(RichText::new("This composition has no other metadata.").weak());
        return;
    }
    ui.label(RichText::new("Others").strong());
    egui::Grid::new("others_grid")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for kv in others {
                row(ui, &kv.key, kv.value.clone());
            }
        });
}

/// Area B: tabs (Metadata / Samples / Patterns). Clicking a name in a list
/// selects it, and area C shows its properties.
pub fn lists(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState, transport: &Transport) {
    let c = &l.project.composition;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut view.tab, ListTab::Metadata, "Metadata");
        ui.selectable_value(
            &mut view.tab,
            ListTab::Samples,
            format!("Samples ({})", c.samples.len()),
        );
        ui.selectable_value(
            &mut view.tab,
            ListTab::Patterns,
            format!("Patterns ({})", c.patterns.len()),
        );
    });
    ui.separator();

    scroll(ui, "list_scroll", |ui| match view.tab {
        ListTab::Metadata => metadata(ui, l),
        ListTab::Samples => sample_list(ui, l, view, transport),
        ListTab::Patterns => pattern_list(ui, l, view, transport),
    });
}

/// Height of a list row: the play button plus a little padding.
const LIST_ROW_HEIGHT: f32 = ICON_BUTTON_SIZE + 4.0;

/// What happened to a list row this frame.
#[derive(Default)]
struct RowOutcome {
    /// The row itself (anywhere but the play button) was clicked.
    selected: bool,
    /// The play button was clicked.
    play: bool,
}

/// One row of a list, a single control: `[play] [color] name`. It takes the
/// whole width, highlights as a whole when selected or hovered, and a click
/// anywhere on it selects it, except on the play button, which only plays.
fn list_row(
    ui: &mut egui::Ui,
    is_selected: bool,
    play_enabled: bool,
    play_tip: &str,
    color: Color32,
    name: RichText,
    scope: &[f32],
) -> RowOutcome {
    let (rect, row) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), LIST_ROW_HEIGHT),
        Sense::click(),
    );
    let visuals = ui.visuals();
    let fill = if is_selected {
        visuals.selection.bg_fill
    } else if row.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    // While the element sounds: its live oscilloscope trace, across the whole
    // row, behind the contents.
    paint_scope(ui.painter(), rect, scope, color.gamma_multiply(0.55));

    // The contents are laid out inside the row, on top of it, so the play
    // button gets its own clicks.
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(4.0, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let play = content
        .add_enabled(play_enabled, IconButton::new(regular::PLAY))
        .on_hover_text(play_tip);
    chip(&mut content, color);
    content.add(egui::Label::new(name).selectable(false));

    RowOutcome {
        selected: row.clicked() && !play.clicked(),
        play: play.clicked(),
    }
}

fn sample_list(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState, transport: &Transport) {
    ui.spacing_mut().item_spacing.y = 2.0;
    for (sample, report) in l.project.composition.samples.iter().zip(&l.sample_reports) {
        let selected = view.selected_sample.as_deref() == Some(sample.id.as_str());
        let color = view.sample_colors.get(&sample.id).copied().unwrap_or(Color32::GRAY);
        let mut name = RichText::new(&sample.id);
        if report.outcome.is_err() {
            name = name.color(ERR_COLOR);
        }
        let can_play = transport.can_preview_sample(&sample.id) && !transport.is_previewing_sample(&sample.id);
        let scope = transport.preview_scope_sample(&sample.id, scope_points(ui.available_width()));
        let row = list_row(ui, selected, can_play, "Play the sample", color, name, &scope);
        if row.play {
            transport.preview_sample(&sample.id);
        }
        if row.selected {
            view.selected_sample = Some(sample.id.clone());
        }
    }
}

fn pattern_list(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState, transport: &Transport) {
    ui.spacing_mut().item_spacing.y = 2.0;
    for pattern in &l.project.composition.patterns {
        let selected = view.selected_pattern.as_deref() == Some(pattern.id.as_str());
        let color = view.pattern_color(l, &pattern.id);
        let can_play = transport.can_preview_pattern(&pattern.id) && !transport.is_previewing_pattern(&pattern.id);
        let scope = transport.preview_scope_pattern(&pattern.id, scope_points(ui.available_width()));
        let row = list_row(ui, selected, can_play, "Play the pattern", color, RichText::new(&pattern.id), &scope);
        if row.play {
            transport.preview_pattern(&pattern.id);
        }
        if row.selected {
            view.selected_pattern = Some(pattern.id.clone());
        }
    }
}

/// Area B: Properties. With the Metadata tab open it shows the metadata's
/// `others`; otherwise the detail of the selected sample or pattern, in two
/// equal columns: on the left all the properties (first what is stored in the
/// `.bm1`, then what is calculated); on the right only where it is used and,
/// for a pattern, its steps.
pub fn properties(ui: &mut egui::Ui, l: &Loaded, view: &ViewState) {
    ui.heading("Properties");
    // Each tab shows the properties of its own selection.
    match view.tab {
        ListTab::Metadata => scroll(ui, "metadata_properties", |ui| others_properties(ui, l)),
        ListTab::Samples => match view.selected_sample.as_deref() {
            None => no_selection(ui),
            Some(id) => two_columns(
                ui,
                "sample",
                |ui| sample_properties(ui, l, id),
                |ui| sample_used(ui, l, id),
            ),
        },
        ListTab::Patterns => match view.selected_pattern.as_deref() {
            None => no_selection(ui),
            Some(id) => two_columns(
                ui,
                "pattern",
                |ui| pattern_properties(ui, l, view, id),
                |ui| pattern_used_and_steps(ui, l, view, id),
            ),
        },
    }
}

fn no_selection(ui: &mut egui::Ui) {
    ui.label(RichText::new("Select an element to see its properties.").weak());
}

/// Two equal columns (a fixed 50% / 50%), each with its own vertical scroll.
fn two_columns(
    ui: &mut egui::Ui,
    id: &str,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    ui.columns(2, |cols| {
        scroll(&mut cols[0], &format!("{id}_stored_scroll"), left);
        scroll(&mut cols[1], &format!("{id}_calculated_scroll"), right);
    });
}

fn find_sample<'a>(
    l: &'a Loaded,
    id: &str,
) -> Option<(&'a bm_format::model::Sample, &'a bm_project::SampleReport)> {
    l.project
        .composition
        .samples
        .iter()
        .zip(&l.sample_reports)
        .find(|(s, _)| s.id == id)
}

/// A sample's properties (left column): first what is stored in the
/// composition, then what is read from its audio.
fn sample_properties(ui: &mut egui::Ui, l: &Loaded, id: &str) {
    let Some((sample, report)) = find_sample(l, id) else { return };
    egui::Grid::new("sample_stored")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            row(ui, "Sample", sample.id.clone());
            row(
                ui,
                "Root note",
                fmt::root_label(sample.root_note.as_deref(), sample.root_octave),
            );
        });

    // The path can be long, so it goes outside the tables and wraps. It is
    // what is stored in the composition, not the resolved path.
    ui.add_space(6.0);
    ui.label(RichText::new("File").weak());
    let stored = l.project.declared_files.get(&sample.id).unwrap_or(&sample.file);
    ui.add(egui::Label::new(stored.as_str()).wrap());

    ui.add_space(8.0);
    egui::Grid::new("sample_calculated")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Status").weak());
            match &report.outcome {
                Ok(()) => ui.label(RichText::new("ok").color(OK_COLOR)),
                Err(err) => ui.label(RichText::new(err.to_string()).color(ERR_COLOR)),
            };
            ui.end_row();
            if let Some(audio) = l.session.as_ref().and_then(|s| s.samples.get(&sample.id)) {
                row(ui, "Length", format!("{:.2} s", audio.duration_seconds()));
                row(ui, "Frames", audio.data.len().to_string());
                row(ui, "Sample rate", format!("{} Hz", audio.sample_rate));
            }
        });
}

/// Right column of a sample: only where it is used.
fn sample_used(ui: &mut egui::Ui, l: &Loaded, id: &str) {
    let users: Vec<&str> = l
        .project
        .composition
        .patterns
        .iter()
        .filter(|p| p.sample == id)
        .map(|p| p.id.as_str())
        .collect();
    ui.label(RichText::new("Used by patterns").strong());
    if users.is_empty() {
        ui.label(RichText::new("none").weak());
    } else {
        ui.label(users.join(", "));
    }
}

/// A pattern's properties (left column): first what is stored in the
/// composition (its steps aside), then what is worked out from them.
fn pattern_properties(ui: &mut egui::Ui, l: &Loaded, view: &ViewState, id: &str) {
    let c = &l.project.composition;
    let Some(pattern) = c.patterns.iter().find(|p| p.id == id) else {
        return;
    };
    let spb = (c.metadata.steps_per_beat as usize).max(1);
    let color = view.pattern_color(l, id);
    egui::Grid::new("pattern_stored")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            row(ui, "Pattern", pattern.id.clone());
            ui.label(RichText::new("Sample").weak());
            ui.horizontal(|ui| {
                chip(ui, color);
                ui.label(&pattern.sample);
            });
            ui.end_row();
        });

    ui.add_space(8.0);
    egui::Grid::new("pattern_calculated")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            row(ui, "Steps", pattern.steps.len().to_string());
            row(ui, "Beats", format!("{:.2}", pattern.steps.len() as f32 / spb as f32));
            if let Some(g) = view.grids.get(id) {
                row(ui, "Sounding steps", g.notes.len().to_string());
                row(ui, "Distinct pitches", g.rows.len().to_string());
            }
        });
}

/// Right column of a pattern: where it is used, then its step grid (which
/// scrolls horizontally on its own).
fn pattern_used_and_steps(ui: &mut egui::Ui, l: &Loaded, view: &ViewState, id: &str) {
    let c = &l.project.composition;
    let spb = (c.metadata.steps_per_beat as usize).max(1);
    let color = view.pattern_color(l, id);

    ui.label(RichText::new("Used in tracks").strong());
    let mut used = false;
    for track in &l.timeline.tracks {
        let total = track.clips.iter().filter(|c| c.pattern_id == id).count();
        if total == 0 {
            continue;
        }
        used = true;
        let repeats = track
            .clips
            .iter()
            .filter(|c| c.pattern_id == id && c.is_repeat)
            .count();
        let extra = if repeats > 0 { format!(" ({repeats} from looping)") } else { String::new() };
        ui.label(format!("{}: {} time(s){}", track.id, total, extra));
    }
    if !used {
        ui.label(RichText::new("not used").weak());
    }

    ui.add_space(8.0);
    ui.label(RichText::new("Steps").strong());
    if let Some(g) = view.grids.get(id) {
        if g.rows.is_empty() {
            ui.label(RichText::new("silent pattern (no notes)").weak());
        } else {
            egui::ScrollArea::horizontal()
                .id_salt("pattern_grid_scroll")
                .show(ui, |ui| grid::draw_pattern_grid(ui, g, spb, color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{load_blocking, LoadOutcome};
    use crate::view::Selection;
    use std::path::Path;

    fn demo() -> Box<Loaded> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1");
        match load_blocking(&path) {
            LoadOutcome::Loaded(l) => l,
            LoadOutcome::Failed { message, .. } => panic!("demo failed to load: {message}"),
        }
    }

    /// Runs two frames in a small window: one to lay out, one with a click
    /// at `pos`. Returns what the row reported on the clicked frame.
    fn click_row_at(pos: egui::Pos2) -> RowOutcome {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 300.0));
        let frame = |events: Vec<egui::Event>| {
            let mut out = RowOutcome::default();
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                out = list_row(ui, false, true, "Play", Color32::RED, RichText::new("kick"), &[]);
            });
            // egui insists that texture updates are handled or cleared.
            output.textures_delta.clear();
            out
        };
        frame(vec![]);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(pos)]);
        frame(vec![button(true)]);
        frame(vec![button(false)])
    }

    #[test]
    fn clicking_a_row_selects_it_but_the_play_button_only_plays() {
        // Blank space at the right of the name: selects the row.
        let row = click_row_at(egui::pos2(300.0, 16.0));
        assert!(row.selected && !row.play);
        // The play button (first thing in the row): plays, does not select.
        let play = click_row_at(egui::pos2(18.0, 16.0));
        assert!(play.play && !play.selected);
    }

    #[test]
    fn each_tab_has_its_own_selection() {
        let l = demo();
        let mut view = ViewState::new(&l);
        assert!(view.selected_sample.is_none() && view.selected_pattern.is_none());
        view.select(Selection::Sample("kick".into()));
        assert_eq!(view.tab, ListTab::Samples);
        view.select(Selection::Pattern("saxA".into()));
        assert_eq!(view.tab, ListTab::Patterns);
        // Both are remembered.
        assert_eq!(view.selected_sample.as_deref(), Some("kick"));
        assert_eq!(view.selected_pattern.as_deref(), Some("saxA"));
        view.select(Selection::None);
        assert!(view.selected_sample.is_none() && view.selected_pattern.is_none());
    }

    #[test]
    fn divider_percentages_stay_in_range_and_leave_room_on_both_sides() {
        let wide = (10.0, 90.0);
        assert_eq!(clamp_percent(5.0, 1000.0, wide), 12.0); // 120 pt minimum wins over 10 %
        assert_eq!(clamp_percent(50.0, 1000.0, wide), 50.0);
        assert_eq!(clamp_percent(99.0, 1000.0, wide), 88.0);
        // The limits of the setting narrow it further.
        assert_eq!(clamp_percent(10.0, 1000.0, (30.0, 50.0)), 30.0);
        assert_eq!(clamp_percent(80.0, 1000.0, (30.0, 50.0)), 50.0);
    }

    #[test]
    fn top_row_draws_for_every_kind_of_selection() {
        let l = demo();
        let mut view = ViewState::new(&l);
        let t = Transport::new(&l);
        for selection in [
            Selection::None,
            Selection::Sample("sax".into()),
            Selection::Pattern("saxA".into()),
            Selection::Pattern("kickA".into()),
        ] {
            view.select(selection);
            egui::__run_test_ui(|ui| top_row(ui, &l, &mut view, &t));
        }
        for tab in [ListTab::Metadata, ListTab::Samples, ListTab::Patterns] {
            view.tab = tab;
            egui::__run_test_ui(|ui| top_row(ui, &l, &mut view, &t));
        }
    }
}
