//! Small pure text-formatting helpers for the UI.

/// `mm:ss.d` (minutes, seconds, tenths), e.g. `00:03.2`.
pub fn duration_mmss(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let tenths_total = (seconds * 10.0).round() as u64;
    let minutes = tenths_total / 600;
    let secs = (tenths_total % 600) / 10;
    let tenths = tenths_total % 10;
    format!("{minutes:02}:{secs:02}.{tenths}")
}

/// A sample's root note as `C4`-style text, `?` when it is not resolved.
pub fn root_label(note: Option<&str>, octave: Option<u8>) -> String {
    match (note, octave) {
        (Some(n), Some(o)) => format!("{n}{o}"),
        _ => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formats_minutes_seconds_and_tenths() {
        assert_eq!(duration_mmss(0.0), "00:00.0");
        assert_eq!(duration_mmss(3.24), "00:03.2");
        assert_eq!(duration_mmss(8.58), "00:08.6");
        assert_eq!(duration_mmss(75.0), "01:15.0");
        assert_eq!(duration_mmss(-4.0), "00:00.0");
    }

    #[test]
    fn root_label_joins_note_and_octave() {
        assert_eq!(root_label(Some("C#"), Some(3)), "C#3");
        assert_eq!(root_label(None, Some(3)), "?");
    }
}
