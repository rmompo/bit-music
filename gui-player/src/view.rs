//! Per-composition view state: what is selected, which tab is open, which
//! tracks are muted, the zoom, and caches derived from the composition.

use std::collections::HashMap;

use eframe::egui::Color32;

use crate::grid::PatternGrid;
use crate::loader::Loaded;
use crate::palette;

/// What the properties panel (C) shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Sample(String),
    Pattern(String),
}

/// The tabs of the list container (B).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListTab {
    #[default]
    Samples,
    Patterns,
}

pub const DEFAULT_STEP_WIDTH: f32 = 16.0;
/// Zoom range of the arrangement, in pixels per step.
pub const STEP_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 6.0..=48.0;

pub struct ViewState {
    pub selection: Selection,
    pub tab: ListTab,
    /// One flag per track, in arrangement order.
    pub muted: Vec<bool>,
    /// Horizontal zoom of the arrangement, in pixels per step.
    pub step_width: f32,
    /// Loop playback when it reaches the end.
    pub looping: bool,
    /// One-shot request to scroll the arrangement horizontally to this
    /// offset (used to keep the playback cursor in view).
    pub scroll_to: Option<f32>,
    /// Piano-roll model of each pattern, by pattern id.
    pub grids: HashMap<String, PatternGrid>,
    /// Color of each sample, by sample id.
    pub sample_colors: HashMap<String, Color32>,
}

impl ViewState {
    pub fn new(l: &Loaded) -> Self {
        let c = &l.project.composition;
        Self {
            selection: Selection::None,
            tab: ListTab::default(),
            muted: vec![false; l.timeline.tracks.len()],
            step_width: DEFAULT_STEP_WIDTH,
            looping: false,
            scroll_to: None,
            grids: c
                .patterns
                .iter()
                .map(|p| (p.id.clone(), PatternGrid::from_steps(&p.steps)))
                .collect(),
            sample_colors: c
                .samples
                .iter()
                .enumerate()
                .map(|(i, s)| (s.id.clone(), palette::sample_color(i)))
                .collect(),
        }
    }

    /// The color of the sample a pattern plays (grey if unknown).
    pub fn pattern_color(&self, l: &Loaded, pattern_id: &str) -> Color32 {
        l.project
            .composition
            .patterns
            .iter()
            .find(|p| p.id == pattern_id)
            .and_then(|p| self.sample_colors.get(&p.sample))
            .copied()
            .unwrap_or(Color32::GRAY)
    }
}
