//! `gui-player.json`: settings and history, stored next to the executable.
//!
//! The file only holds current values. Everything else about a setting —
//! whether the user can edit it, its title, data type, control, limits and
//! default — is defined in `gui-player.schema.json`, which is embedded in
//! the executable. If the file does not exist it is created with the
//! defaults.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The schema of the configuration file.
pub const SCHEMA: &str = include_str!("../gui-player.schema.json");
pub const CONFIG_FILE: &str = "gui-player.json";

pub const MAX_LAST_OPENED: &str = "maxLastOpened";
pub const LANG: &str = "lang";
pub const PATH: &str = "path";
pub const WINDOW_MAXIMIZED: &str = "windowMaximized";
pub const WINDOW_X: &str = "windowX";
pub const WINDOW_Y: &str = "windowY";
pub const WINDOW_WIDTH: &str = "windowWidth";
pub const WINDOW_HEIGHT: &str = "windowHeight";
pub const TABS_WIDTH_PERCENT: &str = "tabsWidthPercent";
pub const ARRANGEMENT_HEIGHT_PERCENT: &str = "arrangementHeightPercent";

// ----- schema ------------------------------------------------------------

/// Whether the user can change a setting in Tools > Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum SettingType {
    #[serde(rename = "USER")]
    User,
    #[serde(rename = "SYSTEM")]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Integer,
    Boolean,
    String,
}

/// The widget Tools > Settings uses to edit a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlType {
    Input,
    Spinner,
    Slider,
    Checkbox,
    Combo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDefinition {
    setting_type: SettingType,
    setting_title: Option<String>,
    setting_description: Option<String>,
    data_type: DataType,
    control_type: Option<ControlType>,
    #[serde(default)]
    values: RawValues,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawValues {
    min_value: Option<i64>,
    max_value: Option<i64>,
    enum_value: Option<Vec<Value>>,
    default_value: Option<Value>,
}

#[derive(Deserialize)]
struct RawSchema {
    settings: BTreeMap<String, RawDefinition>,
}

/// One entry of the schema.
#[derive(Debug, Clone)]
pub struct SettingDef {
    pub key: String,
    pub setting_type: SettingType,
    pub title: String,
    pub description: String,
    pub data_type: DataType,
    pub control_type: Option<ControlType>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    /// The allowed values when the setting is a choice (empty otherwise).
    pub choices: Vec<Value>,
    /// `None` for entries without a meaningful default.
    pub default: Option<Value>,
}

impl SettingDef {
    /// Converts `value` to this setting's type (accepting text such as
    /// `"10"` or `"true"`) and checks it against the limits and the
    /// choices. `None` if it is not acceptable.
    pub fn coerce(&self, value: &Value) -> Option<Value> {
        let converted = match self.data_type {
            DataType::Integer => {
                let n = match value {
                    Value::Number(n) => n.as_i64()?,
                    Value::String(s) => s.trim().parse().ok()?,
                    _ => return None,
                };
                if self.min.is_some_and(|m| n < m) || self.max.is_some_and(|m| n > m) {
                    return None;
                }
                Value::from(n)
            }
            DataType::Boolean => match value {
                Value::Bool(b) => Value::Bool(*b),
                Value::String(s) => Value::Bool(s.trim().parse().ok()?),
                _ => return None,
            },
            DataType::String => match value {
                Value::String(s) => Value::String(s.clone()),
                _ => return None,
            },
        };
        (self.choices.is_empty() || self.choices.contains(&converted)).then_some(converted)
    }
}

/// Every setting the schema defines, in key order.
pub fn definitions() -> &'static [SettingDef] {
    static DEFS: OnceLock<Vec<SettingDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let raw: RawSchema = serde_json::from_str(SCHEMA).expect("the embedded schema is valid");
        raw.settings
            .into_iter()
            .map(|(key, d)| SettingDef {
                title: d.setting_title.unwrap_or_else(|| key.clone()),
                description: d.setting_description.unwrap_or_default(),
                key,
                setting_type: d.setting_type,
                data_type: d.data_type,
                control_type: d.control_type,
                min: d.values.min_value,
                max: d.values.max_value,
                choices: d.values.enum_value.unwrap_or_default(),
                default: d.values.default_value,
            })
            .collect()
    })
}

pub fn definition(key: &str) -> Option<&'static SettingDef> {
    definitions().iter().find(|d| d.key == key)
}

/// The settings the user can edit in Tools > Settings.
pub fn user_definitions() -> impl Iterator<Item = &'static SettingDef> {
    definitions().iter().filter(|d| d.setting_type == SettingType::User)
}

// ----- values ------------------------------------------------------------

/// How the window was left: maximized, or restored with a position (when
/// the system tells it) and a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub maximized: bool,
    pub position: Option<(i32, i32)>,
    pub width: i32,
    pub height: i32,
}

/// Where the dividers were left, as percentages: A's share of the width of
/// A + B, and C's share of the height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerState {
    pub tabs_width_percent: i32,
    pub arrangement_height_percent: i32,
}

/// How far the dividers can be dragged (the limits of their schema
/// entries), in percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerLimits {
    pub tabs_width: (i32, i32),
    pub arrangement_height: (i32, i32),
}

fn limits_of(key: &str) -> (i32, i32) {
    let def = definition(key).expect("divider settings are in the schema");
    (def.min.unwrap_or(10) as i32, def.max.unwrap_or(90) as i32)
}

fn default_int(key: &str) -> i32 {
    definition(key)
        .and_then(|d| d.default.as_ref())
        .and_then(Value::as_i64)
        .expect("this setting has an integer default in the schema") as i32
}

impl Default for DividerLimits {
    fn default() -> Self {
        Self {
            tabs_width: limits_of(TABS_WIDTH_PERCENT),
            arrangement_height: limits_of(ARRANGEMENT_HEIGHT_PERCENT),
        }
    }
}

impl DividerLimits {
    /// Allowed range for A's width share, as `(min, max)`.
    pub fn tabs_width_range(&self) -> (f32, f32) {
        (self.tabs_width.0 as f32, self.tabs_width.1 as f32)
    }

    /// Allowed range for C's height share, as `(min, max)`.
    pub fn arrangement_height_range(&self) -> (f32, f32) {
        (self.arrangement_height.0 as f32, self.arrangement_height.1 as f32)
    }
}

impl Default for DividerState {
    fn default() -> Self {
        Self {
            tabs_width_percent: default_int(TABS_WIDTH_PERCENT),
            arrangement_height_percent: default_int(ARRANGEMENT_HEIGHT_PERCENT),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub settings: Vec<Setting>,
    #[serde(rename = "lastOpened")]
    pub last_opened: Vec<KeyValue>,
}

impl Config {
    /// Default location: next to the running executable.
    pub fn default_path() -> Option<PathBuf> {
        Some(std::env::current_exe().ok()?.parent()?.join(CONFIG_FILE))
    }

    /// Reads `path`, or creates it with the defaults when it does not exist.
    /// Returns a warning when the file could not be used: in that case the
    /// defaults are used in memory and the file is left untouched.
    pub fn load_or_create(path: &Path) -> (Self, Option<String>) {
        if !path.exists() {
            let config = Self::default().with_defaults();
            let warning = config
                .save(path)
                .err()
                .map(|e| format!("could not create {}: {e}", path.display()));
            return (config, warning);
        }
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<Config>(&text).map_err(|e| e.to_string()));
        match parsed {
            Ok(config) => {
                let completed = config.clone().with_defaults();
                let warning = if completed != config {
                    completed.save(path).err().map(|e| format!("could not update {}: {e}", path.display()))
                } else {
                    None
                };
                (completed, warning)
            }
            Err(e) => (
                Self::default().with_defaults(),
                Some(format!("could not read {}: {e}", path.display())),
            ),
        }
    }

    /// Normalizes every setting against the schema: values are converted to
    /// their type, invalid ones are reset to their default (or removed when
    /// there is none), and missing ones are added with their default.
    /// Settings the schema does not know are kept.
    pub fn with_defaults(mut self) -> Self {
        for def in definitions() {
            let position = self.settings.iter().position(|s| s.key == def.key);
            match (position, &def.default) {
                (Some(i), default) => match def.coerce(&self.settings[i].value) {
                    Some(v) => self.settings[i].value = v,
                    None => match default {
                        Some(d) => self.settings[i].value = d.clone(),
                        None => {
                            self.settings.remove(i);
                        }
                    },
                },
                (None, Some(d)) => {
                    self.settings.push(Setting { key: def.key.clone(), value: d.clone() });
                }
                (None, None) => {}
            }
        }
        self
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        text.push('\n');
        std::fs::write(path, text)
    }

    /// The stored value of `key`, or its schema default.
    pub fn value(&self, key: &str) -> Option<Value> {
        self.settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.value.clone())
            .or_else(|| definition(key)?.default.clone())
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.value(key)?.as_i64()
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        self.value(key)?.as_bool()
    }

    pub fn text(&self, key: &str) -> Option<String> {
        self.value(key)?.as_str().map(str::to_string)
    }

    /// The language of the interface (the `lang` setting).
    pub fn language(&self) -> crate::i18n::Lang {
        crate::i18n::Lang::from_setting(&self.text(LANG).unwrap_or_default())
    }

    /// The folder file dialogs start in (the `path` setting), if it is not
    /// empty.
    pub fn files_path(&self) -> Option<String> {
        self.text(PATH).filter(|p| !p.trim().is_empty())
    }

    /// Sets a setting, adding it if it is not there yet.
    pub fn set(&mut self, key: &str, value: Value) {
        match self.settings.iter_mut().find(|s| s.key == key) {
            Some(s) => s.value = value,
            None => self.settings.push(Setting { key: key.to_string(), value }),
        }
    }

    fn remove(&mut self, key: &str) {
        self.settings.retain(|s| s.key != key);
    }

    /// Puts every user-editable setting back to its default.
    pub fn reset_user_settings(&mut self) {
        for def in user_definitions() {
            if let Some(default) = &def.default {
                self.set(&def.key, default.clone());
            }
        }
    }

    /// Takes the user-editable settings and the history from `edited` (the
    /// result of the Settings dialog), then applies their consequences.
    pub fn adopt_user_settings(&mut self, edited: &Config) {
        for def in user_definitions() {
            if let Some(v) = edited.value(&def.key) {
                self.set(&def.key, v);
            }
        }
        self.last_opened = edited.last_opened.clone();
        self.trim_history();
    }

    /// The window state stored in the settings.
    pub fn window(&self) -> WindowState {
        let position = match (self.int(WINDOW_X), self.int(WINDOW_Y)) {
            (Some(x), Some(y)) => Some((x as i32, y as i32)),
            _ => None,
        };
        WindowState {
            maximized: self.flag(WINDOW_MAXIMIZED).unwrap_or(true),
            position,
            width: self.int(WINDOW_WIDTH).unwrap_or(1100) as i32,
            height: self.int(WINDOW_HEIGHT).unwrap_or(720) as i32,
        }
    }

    /// Stores the window state in the settings.
    pub fn set_window(&mut self, w: &WindowState) {
        self.set(WINDOW_MAXIMIZED, Value::from(w.maximized));
        match w.position {
            Some((x, y)) => {
                self.set(WINDOW_X, Value::from(x));
                self.set(WINDOW_Y, Value::from(y));
            }
            None => {
                self.remove(WINDOW_X);
                self.remove(WINDOW_Y);
            }
        }
        self.set(WINDOW_WIDTH, Value::from(w.width));
        self.set(WINDOW_HEIGHT, Value::from(w.height));
    }

    /// The divider positions stored in the settings.
    pub fn dividers(&self) -> DividerState {
        let d = DividerState::default();
        DividerState {
            tabs_width_percent: self.int(TABS_WIDTH_PERCENT).map_or(d.tabs_width_percent, |v| v as i32),
            arrangement_height_percent: self
                .int(ARRANGEMENT_HEIGHT_PERCENT)
                .map_or(d.arrangement_height_percent, |v| v as i32),
        }
    }

    pub fn set_dividers(&mut self, d: &DividerState) {
        self.set(TABS_WIDTH_PERCENT, Value::from(d.tabs_width_percent));
        self.set(ARRANGEMENT_HEIGHT_PERCENT, Value::from(d.arrangement_height_percent));
    }

    /// The maximum size of the history.
    pub fn max_last_opened(&self) -> usize {
        self.int(MAX_LAST_OPENED).filter(|n| *n >= 1).unwrap_or(10) as usize
    }

    /// Drops the oldest history entries beyond the maximum.
    pub fn trim_history(&mut self) {
        self.last_opened.truncate(self.max_last_opened());
    }

    /// Puts `path` first (most recent; moving it if it was already there)
    /// and drops the oldest ones beyond the maximum.
    pub fn record_opened(&mut self, path: &str) {
        self.last_opened.retain(|e| e.key != path);
        self.last_opened.insert(0, KeyValue { key: path.to_string(), value: path.to_string() });
        self.trim_history();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gui-player-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(CONFIG_FILE)
    }

    fn defaults() -> Config {
        Config::default().with_defaults()
    }

    #[test]
    fn the_schema_is_coherent() {
        assert!(!definitions().is_empty());
        for d in definitions() {
            let what = &d.key;
            if let (Some(min), Some(max)) = (d.min, d.max) {
                assert!(min <= max, "{what}: minValue > maxValue");
            }
            if let Some(default) = &d.default {
                assert_eq!(d.coerce(default).as_ref(), Some(default), "{what}: defaultValue is not valid");
            }
            match (d.setting_type, d.control_type) {
                (SettingType::User, None) => panic!("{what}: a USER setting needs a controlType"),
                (SettingType::User, Some(_)) => {
                    assert!(!d.title.is_empty(), "{what}: a USER setting needs a title");
                    assert!(d.default.is_some(), "{what}: a USER setting needs a defaultValue");
                }
                (SettingType::System, _) => {}
            }
            if let Some(control) = d.control_type {
                let compatible = match d.data_type {
                    DataType::Integer => matches!(
                        control,
                        ControlType::Spinner | ControlType::Slider | ControlType::Combo
                    ),
                    DataType::Boolean => control == ControlType::Checkbox,
                    DataType::String => matches!(control, ControlType::Input | ControlType::Combo),
                };
                assert!(compatible, "{what}: {control:?} does not fit {:?}", d.data_type);
                if control == ControlType::Slider {
                    assert!(d.min.is_some() && d.max.is_some(), "{what}: a slider needs minValue and maxValue");
                }
                assert_eq!(control == ControlType::Combo, !d.choices.is_empty(), "{what}: enumValue and combo go together");
            }
        }
    }

    #[test]
    fn the_user_settings_are_language_history_size_and_folder() {
        let user: Vec<&str> = user_definitions().map(|d| d.key.as_str()).collect();
        assert_eq!(user, [LANG, MAX_LAST_OPENED, PATH]);
    }

    #[test]
    fn the_new_user_settings_have_their_documented_choices_and_defaults() {
        let config = defaults();
        assert_eq!(config.language(), crate::i18n::Lang::English);
        assert_eq!(config.max_last_opened(), 10);
        assert_eq!(
            config.files_path().as_deref(),
            Some("C:\\LocalFiles\\proyectos\\personal\\bit-music\\demos\\songs\\")
        );
        // Only 5, 10, 15 or 20 are accepted for the history size; only the
        // two languages for `lang`.
        let mut c = defaults();
        c.set(MAX_LAST_OPENED, Value::from(7));
        c.set(LANG, Value::from("KLINGON"));
        let c = c.with_defaults();
        assert_eq!(c.max_last_opened(), 10);
        assert_eq!(c.language(), crate::i18n::Lang::English);
        let mut c = defaults();
        c.set(MAX_LAST_OPENED, Value::from(15));
        c.set(LANG, Value::from("SPANISH"));
        let c = c.with_defaults();
        assert_eq!((c.max_last_opened(), c.language()), (15, crate::i18n::Lang::Spanish));
    }

    #[test]
    fn entries_without_a_default_are_not_added() {
        let config = defaults();
        assert_eq!(config.value(WINDOW_X), None);
        assert_eq!(config.window().position, None);
        assert_eq!(config.max_last_opened(), 10);
    }

    #[test]
    fn values_are_typed_and_text_from_older_files_is_accepted() {
        let config: Config = serde_json::from_str(
            r#"{"settings":[{"key":"maxLastOpened","value":"15"},{"key":"windowMaximized","value":"false"}]}"#,
        )
        .unwrap();
        let config = config.with_defaults();
        assert_eq!(config.value(MAX_LAST_OPENED), Some(Value::from(15)));
        assert_eq!(config.value(WINDOW_MAXIMIZED), Some(Value::Bool(false)));
    }

    #[test]
    fn the_window_opens_maximized_by_default_and_round_trips() {
        let mut config = defaults();
        let w = config.window();
        assert!(w.maximized);
        assert_eq!((w.width, w.height, w.position), (1100, 720, None));

        let restored = WindowState { maximized: false, position: Some((-20, 40)), width: 900, height: 600 };
        config.set_window(&restored);
        assert_eq!(config.window(), restored);
        assert_eq!(config.clone().with_defaults().window(), restored);
        // A too-small size is reset to the default.
        config.set(WINDOW_WIDTH, Value::from(10));
        assert_eq!(config.with_defaults().window().width, 1100);
    }

    #[test]
    fn dividers_default_and_are_limited_by_their_schema_entries() {
        let mut config = defaults();
        assert_eq!(config.dividers(), DividerState { tabs_width_percent: 30, arrangement_height_percent: 50 });
        let limits = DividerLimits::default();
        assert_eq!(limits.tabs_width_range(), (30.0, 50.0));
        assert_eq!(limits.arrangement_height_range(), (50.0, 75.0));

        let moved = DividerState { tabs_width_percent: 45, arrangement_height_percent: 62 };
        config.set_dividers(&moved);
        assert_eq!(config.clone().with_defaults().dividers(), moved);
        // Outside the limits: back to the default.
        config.set(TABS_WIDTH_PERCENT, Value::from(60));
        config.set(ARRANGEMENT_HEIGHT_PERCENT, Value::from(20));
        assert_eq!(config.with_defaults().dividers(), DividerState::default());
    }

    #[test]
    fn a_missing_file_is_created_with_the_defaults() {
        let path = temp_file("create");
        let (config, warning) = Config::load_or_create(&path);
        assert!(warning.is_none());
        assert_eq!(config.int(MAX_LAST_OPENED), Some(10));
        assert!(config.last_opened.is_empty());
        let on_disk: Config = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk, config);
    }

    #[test]
    fn invalid_settings_fall_back_to_their_default_and_unknown_ones_are_kept() {
        let path = temp_file("defaults");
        std::fs::write(&path, r#"{"settings":[{"key":"maxLastOpened","value":"zero"},{"key":"other","value":"x"}]}"#).unwrap();
        let (config, _) = Config::load_or_create(&path);
        assert_eq!(config.int(MAX_LAST_OPENED), Some(10));
        assert_eq!(config.value("other"), Some(Value::from("x")));
    }

    #[test]
    fn a_corrupt_file_is_reported_and_left_untouched() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{ nope").unwrap();
        let (config, warning) = Config::load_or_create(&path);
        assert!(warning.unwrap().contains("could not read"));
        assert_eq!(config.max_last_opened(), 10);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ nope");
    }

    #[test]
    fn history_is_newest_first_up_to_the_limit_without_duplicates() {
        let mut config = defaults();
        config.set(MAX_LAST_OPENED, Value::from(3));
        for p in ["a", "b", "c", "a", "d"] {
            config.record_opened(p);
        }
        let keys: Vec<&str> = config.last_opened.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["d", "a", "c"]);
        assert!(config.last_opened.iter().all(|e| e.key == e.value));
    }

    #[test]
    fn settings_dialog_results_are_adopted_and_reset_all_restores_defaults() {
        let mut config = defaults();
        for p in ["a", "b", "c"] {
            config.record_opened(p);
        }
        let mut edited = config.clone();
        edited.set(MAX_LAST_OPENED, Value::from(2));
        config.adopt_user_settings(&edited);
        assert_eq!(config.max_last_opened(), 2);
        assert_eq!(config.last_opened.len(), 2); // trimmed by the new limit

        edited.reset_user_settings();
        assert_eq!(edited.max_last_opened(), 10);
        // System state is not touched by "reset all".
        let mut moved = defaults();
        moved.set(TABS_WIDTH_PERCENT, Value::from(45));
        moved.reset_user_settings();
        assert_eq!(moved.int(TABS_WIDTH_PERCENT), Some(45));
    }
}
