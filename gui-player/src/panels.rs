//! The top row of the window: A (metadata), B (tabbed lists of samples and
//! patterns, names only) and C (properties of whatever is selected in B or
//! in the arrangement).

use eframe::egui::{self, Color32, RichText, Sense, Vec2};

use crate::fmt;
use crate::grid;
use crate::loader::Loaded;
use crate::view::{ListTab, Selection, ViewState};

pub const OK_COLOR: Color32 = Color32::from_rgb(90, 190, 110);
pub const ERR_COLOR: Color32 = Color32::from_rgb(230, 90, 90);

/// Minimum width of the key column in property tables, so tables stacked
/// in one panel line their values up.
const MIN_KEY_WIDTH: f32 = 130.0;

/// A | B | C, each with its own vertical scroll.
pub fn top_row(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState) {
    ui.columns(3, |cols| {
        scroll(&mut cols[0], "metadata_scroll", |ui| metadata(ui, l));
        lists(&mut cols[1], l, view);
        scroll(&mut cols[2], "properties_scroll", |ui| properties(ui, l, view));
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

/// Area A: composition metadata.
pub fn metadata(ui: &mut egui::Ui, l: &Loaded) {
    let m = &l.project.composition.metadata;
    ui.heading("Metadata");
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

    if !m.others.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("Others").strong());
        egui::Grid::new("others_grid")
            .num_columns(2)
            .min_col_width(MIN_KEY_WIDTH)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for kv in &m.others {
                    row(ui, &kv.key, kv.value.clone());
                }
            });
    }
}

/// Area B: tabs (Samples / Patterns) over a list of names. Clicking a name
/// selects it, and area C shows its properties.
pub fn lists(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState) {
    let c = &l.project.composition;
    ui.horizontal(|ui| {
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
        ListTab::Samples => sample_list(ui, l, view),
        ListTab::Patterns => pattern_list(ui, l, view),
    });
}

fn sample_list(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState) {
    for (sample, report) in l.project.composition.samples.iter().zip(&l.sample_reports) {
        let selected = matches!(&view.selection, Selection::Sample(id) if *id == sample.id);
        let color = view.sample_colors.get(&sample.id).copied().unwrap_or(Color32::GRAY);
        ui.horizontal(|ui| {
            chip(ui, color);
            let mut text = RichText::new(&sample.id);
            if report.outcome.is_err() {
                text = text.color(ERR_COLOR);
            }
            if ui.selectable_label(selected, text).clicked() {
                view.selection = Selection::Sample(sample.id.clone());
            }
        });
    }
}

fn pattern_list(ui: &mut egui::Ui, l: &Loaded, view: &mut ViewState) {
    for pattern in &l.project.composition.patterns {
        let selected = matches!(&view.selection, Selection::Pattern(id) if *id == pattern.id);
        let color = view.pattern_color(l, &pattern.id);
        ui.horizontal(|ui| {
            chip(ui, color);
            if ui.selectable_label(selected, &pattern.id).clicked() {
                view.selection = Selection::Pattern(pattern.id.clone());
            }
        });
    }
}

/// Area C: the detail of the selected sample or pattern.
pub fn properties(ui: &mut egui::Ui, l: &Loaded, view: &ViewState) {
    ui.heading("Properties");
    match &view.selection {
        Selection::None => {
            ui.label(RichText::new("Select a sample or a pattern to see its properties.").weak());
        }
        Selection::Sample(id) => sample_properties(ui, l, id),
        Selection::Pattern(id) => pattern_properties(ui, l, view, id),
    }
}

fn sample_properties(ui: &mut egui::Ui, l: &Loaded, id: &str) {
    let c = &l.project.composition;
    let Some((sample, report)) = c
        .samples
        .iter()
        .zip(&l.sample_reports)
        .find(|(s, _)| s.id == id)
    else {
        return;
    };

    egui::Grid::new("sample_properties")
        .num_columns(2)
        .min_col_width(MIN_KEY_WIDTH)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            row(ui, "Sample", sample.id.clone());
            ui.label(RichText::new("Status").weak());
            match &report.outcome {
                Ok(()) => ui.label(RichText::new("ok").color(OK_COLOR)),
                Err(err) => ui.label(RichText::new(err.to_string()).color(ERR_COLOR)),
            };
            ui.end_row();
            row(
                ui,
                "Root note",
                fmt::root_label(sample.root_note.as_deref(), sample.root_octave),
            );
            if let Some(audio) = l.session.as_ref().and_then(|s| s.samples.get(&sample.id)) {
                row(ui, "Length", format!("{:.2} s", audio.duration_seconds()));
                row(ui, "Frames", audio.data.len().to_string());
                row(ui, "Sample rate", format!("{} Hz", audio.sample_rate));
            }
        });

    // The path can be long, so it goes outside the table and wraps.
    ui.add_space(6.0);
    ui.label(RichText::new("File").weak());
    ui.add(egui::Label::new(sample.file.as_str()).wrap());

    let users: Vec<&str> = c
        .patterns
        .iter()
        .filter(|p| p.sample == id)
        .map(|p| p.id.as_str())
        .collect();
    ui.add_space(8.0);
    ui.label(RichText::new("Used by patterns").strong());
    if users.is_empty() {
        ui.label(RichText::new("none").weak());
    } else {
        ui.label(users.join(", "));
    }
}

fn pattern_properties(ui: &mut egui::Ui, l: &Loaded, view: &ViewState, id: &str) {
    let c = &l.project.composition;
    let Some(pattern) = c.patterns.iter().find(|p| p.id == id) else {
        return;
    };
    let spb = (c.metadata.steps_per_beat as usize).max(1);
    let color = view.pattern_color(l, id);
    let grid_model = view.grids.get(id);

    egui::Grid::new("pattern_properties")
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
            row(ui, "Steps", pattern.steps.len().to_string());
            row(ui, "Beats", format!("{:.2}", pattern.steps.len() as f32 / spb as f32));
            if let Some(g) = grid_model {
                row(ui, "Sounding steps", g.notes.len().to_string());
                row(ui, "Distinct pitches", g.rows.len().to_string());
            }
        });

    ui.add_space(8.0);
    ui.label(RichText::new("Steps").strong());
    if let Some(g) = grid_model {
        if g.rows.is_empty() {
            ui.label(RichText::new("silent pattern (no notes)").weak());
        } else {
            egui::ScrollArea::horizontal()
                .id_salt("pattern_grid_scroll")
                .show(ui, |ui| grid::draw_pattern_grid(ui, g, spb, color));
        }
    }
    ui.add_space(8.0);
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{load_blocking, LoadOutcome};
    use std::path::Path;

    fn demo() -> Box<Loaded> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../player/demos/songs/song1.bm1");
        match load_blocking(&path) {
            LoadOutcome::Loaded(l) => l,
            LoadOutcome::Failed { message, .. } => panic!("demo failed to load: {message}"),
        }
    }

    #[test]
    fn top_row_draws_for_every_kind_of_selection() {
        let l = demo();
        let mut view = ViewState::new(&l);
        for selection in [
            Selection::None,
            Selection::Sample("sax".into()),
            Selection::Pattern("saxA".into()),
            Selection::Pattern("kickA".into()),
        ] {
            view.selection = selection;
            egui::__run_test_ui(|ui| top_row(ui, &l, &mut view));
        }
        view.tab = ListTab::Patterns;
        egui::__run_test_ui(|ui| top_row(ui, &l, &mut view));
    }
}
