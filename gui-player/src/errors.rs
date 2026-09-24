//! The error log: what went wrong while the app has been running, newest
//! first (a stack). The footer shows the latest and a dialog lists them all.

use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorEntry {
    /// Identifies the entry, to remove it from the list.
    pub id: u64,
    pub time: SystemTime,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct ErrorLog {
    /// Newest first.
    entries: Vec<ErrorEntry>,
    next_id: u64,
}

impl ErrorLog {
    /// Puts an error on top of the stack. An error identical to the one on
    /// top is not repeated (a fault that persists would fill the list).
    /// Returns whether it was added.
    pub fn push(&mut self, text: impl Into<String>) -> bool {
        let text = text.into();
        if self.entries.first().is_some_and(|e| e.text == text) {
            return false;
        }
        self.entries.insert(
            0,
            ErrorEntry { id: self.next_id, time: SystemTime::now(), text },
        );
        self.next_id += 1;
        true
    }

    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The most recent error, if any.
    pub fn latest(&self) -> Option<&ErrorEntry> {
        self.entries.first()
    }

    pub fn entries(&self) -> &[ErrorEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// `24/09/2026 13:05:22`: date and time in the system's local time zone.
pub fn format_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%d/%m/%Y %H:%M:%S")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn it_is_a_stack_newest_first() {
        let mut log = ErrorLog::default();
        assert!(log.is_empty() && log.latest().is_none());
        log.push("first");
        log.push("second");
        log.push("third");
        let texts: Vec<&str> = log.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["third", "second", "first"]);
        assert_eq!(log.latest().unwrap().text, "third");
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn a_repeat_of_the_latest_is_not_added_but_an_older_one_can_come_back() {
        let mut log = ErrorLog::default();
        assert!(log.push("a"));
        assert!(!log.push("a"));
        assert!(log.push("b"));
        assert!(log.push("a")); // not the latest any more
        assert_eq!(log.len(), 3);
    }

    #[test]
    fn entries_are_removed_one_by_one_or_all_at_once() {
        let mut log = ErrorLog::default();
        log.push("a");
        log.push("b");
        log.push("c");
        let middle = log.entries()[1].id;
        log.remove(middle);
        let texts: Vec<&str> = log.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["c", "a"]);
        // Ids are not reused, so a removed entry cannot be confused with a new one.
        log.push("d");
        assert!(log.entries().iter().all(|e| e.id != middle));
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn the_time_is_local_day_month_year_and_hours_minutes_seconds() {
        let text = format_time(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 3 + 13 * 3600 + 5 * 60 + 22));
        // DD/MM/YYYY HH24:MM:SS. The hour (and, near midnight, the day)
        // depend on the machine's time zone; the shape and the seconds do not.
        let (date, time) = text.split_once(' ').unwrap_or_else(|| panic!("{text}"));
        let shape = |s: &str, sep: char, widths: &[usize]| {
            let parts: Vec<&str> = s.split(sep).collect();
            parts.len() == widths.len()
                && parts.iter().zip(widths).all(|(p, w)| p.len() == *w && p.chars().all(|c| c.is_ascii_digit()))
        };
        assert!(shape(date, '/', &[2, 2, 4]), "{text}");
        assert!(shape(time, ':', &[2, 2, 2]), "{text}");
        assert!(text.ends_with(":22"), "{text}");
        assert!(date.ends_with("/1970"), "{text}");
        // What the machine's clock says now (either side of the call, in case
        // a minute changes in between).
        let clock = || chrono::Local::now().format("%d/%m/%Y %H:%M").to_string();
        let before = clock();
        let shown = format_time(SystemTime::now())[..16].to_string();
        let after = clock();
        assert!(shown == before || shown == after, "{shown} vs {before}/{after}");
    }
}
