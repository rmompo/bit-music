//! Colors used to tell samples (and therefore the patterns that use them)
//! apart everywhere in the UI.

use eframe::egui::Color32;

const SAMPLE_COLORS: [Color32; 8] = [
    Color32::from_rgb(86, 156, 214),  // blue
    Color32::from_rgb(220, 140, 70),  // orange
    Color32::from_rgb(110, 190, 120), // green
    Color32::from_rgb(200, 100, 160), // pink
    Color32::from_rgb(170, 140, 220), // purple
    Color32::from_rgb(210, 190, 80),  // yellow
    Color32::from_rgb(80, 190, 190),  // teal
    Color32::from_rgb(200, 100, 100), // red
];

/// The color of the `index`-th sample (cycles if there are more than 8).
pub fn sample_color(index: usize) -> Color32 {
    SAMPLE_COLORS[index % SAMPLE_COLORS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_distinct_and_cycle() {
        assert_ne!(sample_color(0), sample_color(1));
        assert_eq!(sample_color(0), sample_color(8));
    }
}
