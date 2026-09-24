//! Per-composition view state: what is selected, which tab is open, which
//! tracks are muted, the zoom, the master volume, and caches derived from the composition.

use std::collections::HashMap;

use eframe::egui::Color32;

use crate::config::DividerLimits;
use crate::grid::PatternGrid;
use crate::loader::Loaded;
use crate::palette;

/// Something that can be selected in one of the lists.
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
    Metadata,
    Samples,
    Patterns,
}

pub const DEFAULT_STEP_WIDTH: f32 = 16.0;
/// Zoom range of the arrangement, in pixels per step.
pub const STEP_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 6.0..=48.0;

pub struct ViewState {
    /// The sample selected in the Samples tab.
    pub selected_sample: Option<String>,
    /// The pattern selected in the Patterns tab.
    pub selected_pattern: Option<String>,
    pub tab: ListTab,
    /// One flag per track, in arrangement order.
    pub muted: Vec<bool>,
    /// Horizontal zoom of the arrangement, in pixels per step.
    pub step_width: f32,
    /// Loop playback when it reaches the end.
    pub looping: bool,
    /// Master volume, 0.0..=1.0. Muted means 0.
    pub volume: f32,
    /// The last volume that was not 0: what un-muting goes back to.
    pub last_volume: f32,
    /// Vertical divider: percentage of the top row's width taken by A.
    pub tabs_width_percent: f32,
    /// Horizontal divider: percentage of the height taken by C (the
    /// arrangement); A + B get the rest.
    pub arrangement_height_percent: f32,
    /// How far each divider can be dragged (from the schema).
    pub divider_limits: DividerLimits,
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
            selected_sample: None,
            selected_pattern: None,
            tab: ListTab::default(),
            muted: vec![false; l.timeline.tracks.len()],
            step_width: DEFAULT_STEP_WIDTH,
            looping: false,
            volume: 1.0,
            last_volume: 1.0,
            tabs_width_percent: 30.0,
            arrangement_height_percent: 50.0,
            divider_limits: DividerLimits::default(),
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

    /// Selects `selection` in its own tab and opens that tab (`None` clears
    /// both). Each tab keeps its own selection.
    pub fn select(&mut self, selection: Selection) {
        match selection {
            Selection::None => {
                self.selected_sample = None;
                self.selected_pattern = None;
            }
            Selection::Sample(id) => {
                self.selected_sample = Some(id);
                self.tab = ListTab::Samples;
            }
            Selection::Pattern(id) => {
                self.selected_pattern = Some(id);
                self.tab = ListTab::Patterns;
            }
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
