//! The interface's texts, in every language it supports.
//!
//! Each language is a flat JSON file of `key: text` in `gui-player/i18n/`,
//! embedded in the executable. Texts with values use `{name}` placeholders
//! (see [`tf`]). A key missing from the current language falls back to
//! English, and then to the key itself, so a gap never breaks the interface.
//! The current language is per thread: the interface runs on one thread, and
//! tests on theirs, so they cannot disturb each other.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The languages of the interface. The names are the values of the `lang`
/// setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    English,
    Spanish,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::Spanish];

    /// The value of the `lang` setting for this language.
    pub fn setting_value(self) -> &'static str {
        match self {
            Lang::English => "ENGLISH",
            Lang::Spanish => "SPANISH",
        }
    }

    /// The language a `lang` setting value names (English if unknown).
    pub fn from_setting(value: &str) -> Lang {
        Lang::ALL
            .into_iter()
            .find(|l| l.setting_value() == value)
            .unwrap_or_default()
    }

    fn json(self) -> &'static str {
        match self {
            Lang::English => include_str!("../i18n/en.json"),
            Lang::Spanish => include_str!("../i18n/es.json"),
        }
    }
}

type Table = HashMap<String, String>;

fn table(lang: Lang) -> &'static Table {
    static ENGLISH: OnceLock<Table> = OnceLock::new();
    static SPANISH: OnceLock<Table> = OnceLock::new();
    let cell = match lang {
        Lang::English => &ENGLISH,
        Lang::Spanish => &SPANISH,
    };
    cell.get_or_init(|| serde_json::from_str(lang.json()).expect("the embedded texts are valid JSON"))
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::English) };
}

pub fn set_language(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

pub fn language() -> Lang {
    CURRENT.with(Cell::get)
}

/// The text for `key` in the current language, if there is one (falling
/// back to English).
pub fn lookup(key: &str) -> Option<&'static str> {
    table(language())
        .get(key)
        .or_else(|| table(Lang::English).get(key))
        .map(String::as_str)
}

/// The text for `key`; the key itself if no language has it.
pub fn t(key: &'static str) -> &'static str {
    lookup(key).unwrap_or(key)
}

/// The text for `key` with each `{name}` replaced by its value.
pub fn tf(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut text = t(key).to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<String> {
        text.split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string()))
            .collect()
    }

    #[test]
    fn every_language_has_the_same_keys_and_placeholders() {
        let english = table(Lang::English);
        for lang in Lang::ALL {
            let other = table(lang);
            let missing: Vec<_> = english.keys().filter(|k| !other.contains_key(*k)).collect();
            let extra: Vec<_> = other.keys().filter(|k| !english.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{lang:?} lacks {missing:?}");
            assert!(extra.is_empty(), "{lang:?} has unknown keys {extra:?}");
            for (key, text) in english {
                assert_eq!(placeholders(text), placeholders(&other[key]), "{lang:?}: placeholders of {key}");
                assert!(!other[key].trim().is_empty(), "{lang:?}: {key} is empty");
            }
        }
    }

    #[test]
    fn every_key_used_in_the_sources_exists() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("i18n.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for marker in ["t(\"", "tf(\""] {
                for (at, _) in source.match_indices(marker) {
                    // A call to `t`/`tf`, not the end of another name (`set(`).
                    let before = source[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let key = source[at + marker.len()..].split('"').next().unwrap();
                    assert!(lookup(key).is_some(), "{}: unknown text key `{key}`", path.display());
                }
            }
        }
    }

    #[test]
    fn the_language_switches_and_falls_back_to_english_then_to_the_key() {
        set_language(Lang::English);
        assert_eq!(t("menu.file"), "File");
        set_language(Lang::Spanish);
        assert_eq!(t("menu.file"), "Archivo");
        assert_eq!(tf("status.samples", &[("ok", "3"), ("total", "5")]), "3/5 samples");
        assert_eq!(t("no.such.key"), "no.such.key");
        set_language(Lang::English);
    }

    #[test]
    fn settings_values_map_to_languages() {
        assert_eq!(Lang::from_setting("SPANISH"), Lang::Spanish);
        assert_eq!(Lang::from_setting("ENGLISH"), Lang::English);
        assert_eq!(Lang::from_setting("KLINGON"), Lang::English);
        assert_eq!(Lang::Spanish.setting_value(), "SPANISH");
    }
}
