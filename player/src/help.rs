//! Loads and renders the `bm help` text from an external, user-editable
//! JSON file (`bm.hlp`), so the help content can be tweaked without
//! recompiling — and so it stays structured (machine-parseable) rather
//! than being a raw blob of prose.
//!
//! Looks for `bm.hlp` next to the running executable first; if it's
//! missing, unreadable, or not valid JSON, falls back to the copy embedded
//! into the binary at build time (a warning is printed to stderr in the
//! invalid-JSON case, since that likely means a typo the user should fix).

use std::io::IsTerminal;

use serde::Deserialize;

const EMBEDDED_HELP: &str = include_str!("../bm.hlp");

/// Fallback terminal width when it can't be detected (piped output, no
/// real terminal, etc).
const DEFAULT_WIDTH: usize = 80;
/// Never wrap narrower than this, even on a tiny/misreported terminal.
const MIN_WIDTH: usize = 40;

#[derive(Debug, Deserialize)]
struct HelpDoc {
    name: String,
    description: String,
    usage: String,
    commands: Vec<HelpCommand>,
    #[serde(default)]
    examples: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct HelpCommand {
    /// Kept for structure/machine consumption (e.g. matching by name)
    /// even though only `usage` is rendered to the terminal.
    #[allow(dead_code)]
    name: String,
    usage: String,
    description: String,
    #[serde(default)]
    flags: Vec<HelpFlag>,
}

#[derive(Debug, Deserialize)]
struct HelpFlag {
    name: String,
    description: String,
}

pub fn print_help() {
    let doc = load_external_help().unwrap_or_else(|| {
        // The embedded copy is checked at build time (see the test below),
        // so this should never fail in practice.
        serde_json::from_str(EMBEDDED_HELP).expect("embedded bm.hlp is valid JSON")
    });
    render(&doc);
}

fn load_external_help() -> Option<HelpDoc> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let raw = std::fs::read_to_string(dir.join("bm.hlp")).ok()?;
    match serde_json::from_str(&raw) {
        Ok(doc) => Some(doc),
        Err(err) => {
            eprintln!("Warning: bm.hlp is not valid JSON ({err}), using the built-in help text instead");
            None
        }
    }
}

/// Minimal ANSI painter: no-ops when stdout isn't a real terminal or
/// `NO_COLOR` is set, so piped/redirected output (and any terminal that
/// opts out) stays plain text.
struct Painter {
    enabled: bool,
}

impl Painter {
    fn detect() -> Self {
        let enabled =
            std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        Self { enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn title(&self, text: &str) -> String {
        self.paint("1", text) // bold
    }

    fn heading(&self, text: &str) -> String {
        self.paint("1;33", text) // bold yellow
    }

    fn command(&self, text: &str) -> String {
        self.paint("1;36", text) // bold cyan
    }

    fn flag(&self, text: &str) -> String {
        self.paint("32", text) // green
    }
}

/// Detects the terminal width (via `crossterm`), falling back to
/// `DEFAULT_WIDTH` when it can't be determined (piped output, no tty).
fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(cols, _rows)| cols as usize)
        .unwrap_or(DEFAULT_WIDTH)
        .max(MIN_WIDTH)
}

/// Greedy word-wraps `text` to fit within `width` columns, prefixing every
/// line (including the first) with `indent`. Always returns at least one
/// line, so empty descriptions still produce a bare indent.
fn wrap(text: &str, indent: &str, width: usize) -> Vec<String> {
    let avail = width.saturating_sub(indent.chars().count()).max(10);

    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let extra = if current.is_empty() { 0 } else { 1 };
        if current.chars().count() + extra + word.chars().count() > avail && !current.is_empty()
        {
            lines.push(format!("{indent}{current}"));
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    lines.push(format!("{indent}{current}"));
    lines
}

fn render(doc: &HelpDoc) {
    let p = Painter::detect();
    let width = terminal_width();

    println!("{} - {}", p.title(&doc.name), doc.description);
    println!();
    println!("{}", p.heading("USAGE:"));
    println!("    {}", doc.usage);
    println!();
    println!("{}", p.heading("COMMANDS:"));
    for cmd in &doc.commands {
        println!("    {}", p.command(&cmd.usage));
        for line in wrap(&cmd.description, "        ", width) {
            println!("{line}");
        }
        for flag in &cmd.flags {
            println!("            {}", p.flag(&flag.name));
            for line in wrap(&flag.description, "                ", width) {
                println!("{line}");
            }
        }
        println!();
    }
    if !doc.examples.is_empty() {
        println!("{}", p.heading("EXAMPLES:"));
        for example in &doc.examples {
            println!("    {example}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_help_is_valid_json() {
        let doc: HelpDoc =
            serde_json::from_str(EMBEDDED_HELP).expect("bm.hlp must be valid JSON");
        assert!(!doc.commands.is_empty());
    }

    #[test]
    fn wrap_keeps_short_text_on_one_line() {
        let lines = wrap("short text", "  ", 80);
        assert_eq!(lines, vec!["  short text".to_string()]);
    }

    #[test]
    fn wrap_breaks_long_text_to_fit_width_with_indent() {
        let text = "one two three four five six seven eight nine ten";
        let indent = "    ";
        let width = 20;
        let lines = wrap(text, indent, width);
        assert!(lines.len() > 1, "expected multiple lines, got {lines:?}");
        for line in &lines {
            assert!(
                line.chars().count() <= width,
                "line exceeds width {width}: '{line}' ({} chars)",
                line.chars().count()
            );
            assert!(line.starts_with(indent));
        }
        // no words lost or reordered
        let rejoined: String = lines
            .iter()
            .map(|l| l.trim_start_matches(indent))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(rejoined, text);
    }

    #[test]
    fn wrap_never_panics_on_empty_text() {
        let lines = wrap("", "  ", 80);
        assert_eq!(lines, vec!["  ".to_string()]);
    }
}
