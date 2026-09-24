//! Application state and the main UI loop.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;

use crate::config::{Config, DividerState};
use crate::dialogs::{self, Dialog};
use crate::errors::ErrorLog;
use crate::i18n::{self, t, tf};
use crate::chrome::{self, StatusLine, StatusSliders};
use crate::loader::{self, file_name, LoadOutcome, Loaded};
use crate::panels;
use crate::screenshot::{self, ScreenshotJob};
use crate::transport::{self, Transport};
use crate::view::{self, ViewState};
use crate::widgets;
use crate::arrangement;

enum State {
    Empty,
    Loading {
        path: PathBuf,
        rx: Receiver<LoadOutcome>,
    },
    Ready(Box<Ready>),
    Failed {
        path: PathBuf,
        message: String,
    },
}

/// A composition that is open: what was loaded plus how it is being viewed.
struct Ready {
    loaded: Loaded,
    view: ViewState,
    transport: Transport,
    /// The audio stream's failure has been put in the error log.
    stream_error_logged: bool,
}

pub struct PlayerApp {
    state: State,
    last_title: String,
    /// The modal dialog currently open, if any.
    dialog: Option<Dialog>,
    /// The user confirmed quitting, so the next close request goes through.
    quit_confirmed: bool,
    /// The text of the open [`Dialog::Notice`].
    notice: String,
    /// What went wrong, newest first.
    errors: ErrorLog,
    /// The copy of the configuration being edited in Tools > Settings.
    settings_draft: Option<Config>,
    /// Settings and history (`gui-player.json`).
    config: Config,
    /// Where the configuration is stored; `None` means memory only.
    config_path: Option<PathBuf>,
    /// The configuration changed (window state) and is waiting to be saved.
    config_dirty_since: Option<std::time::Instant>,
    /// Developer aid, only set through `BM_GUI_SCREENSHOT`.
    screenshot: Option<ScreenshotJob>,
}

const APP_TITLE: &str = dialogs::PRODUCT;

impl PlayerApp {
    /// `initial` is a composition to open at startup (e.g. from the command line).
    /// `config_path` is where `gui-player.json` lives (created if missing);
    /// `None` keeps the configuration in memory only.
    pub fn new(ctx: &egui::Context, initial: Option<PathBuf>, config_path: Option<PathBuf>) -> Self {
        chrome::install_icon_font(ctx);
        let mut errors = ErrorLog::default();
        let config = match &config_path {
            Some(path) => {
                let (config, warning) = Config::load_or_create(path);
                if let Some(warning) = warning {
                    eprintln!("{warning}");
                    errors.push(warning);
                }
                config
            }
            None => Config::default().with_defaults(),
        };
        i18n::set_language(config.language());
        let mut app = Self {
            state: State::Empty,
            last_title: String::new(),
            dialog: screenshot::initial_dialog(),
            // The developer screenshot mode closes the window by itself when it
            // is done, without asking.
            quit_confirmed: screenshot::ScreenshotJob::from_env().is_some(),
            notice: String::new(),
            errors,
            settings_draft: None,
            config,
            config_path,
            config_dirty_since: None,
            screenshot: ScreenshotJob::from_env(),
        };
        if app.dialog == Some(Dialog::Settings) {
            app.settings_draft = Some(app.config.clone());
        }
        if let Some(path) = initial {
            app.open(ctx, path);
        }
        app
    }

    /// Starts loading `path` in the background, replacing whatever is open.
    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let rx = loader::spawn_load(path.clone(), ctx.clone());
        self.state = State::Loading { path, rx };
    }

    /// Moves a finished background load into the state. If the loading
    /// thread died without answering (e.g. it panicked), the app reports a
    /// failure instead of showing "Loading…" forever.
    fn poll_loading(&mut self) {
        let State::Loading { path, rx } = &self.state else {
            return;
        };
        let mut opened = None;
        let next = match rx.try_recv() {
            Ok(LoadOutcome::Loaded(loaded)) => {
                opened = Some(path.clone());
                let mut view = ViewState::new(&loaded);
                let dividers = self.config.dividers();
                // The stored positions are already within the schema's limits
                // (`with_defaults`), but clamp anyway.
                let (a_lo, a_hi) = view.divider_limits.tabs_width_range();
                let (c_lo, c_hi) = view.divider_limits.arrangement_height_range();
                view.tabs_width_percent = (dividers.tabs_width_percent as f32).clamp(a_lo, a_hi);
                view.arrangement_height_percent =
                    (dividers.arrangement_height_percent as f32).clamp(c_lo, c_hi);
                if let Some(volume) = screenshot::initial_volume() {
                    view.volume = volume;
                }
                if let Some((selection, tab)) = screenshot::initial_selection() {
                    view.select(selection);
                    view.tab = tab;
                }
                let transport = Transport::new(&loaded);
                if let Some(issue) = transport.error() {
                    self.errors.push(tf("transport.no_audio", &[("reason", &issue.text())]));
                }
                match screenshot::initial_preview() {
                    Some(view::Selection::Sample(id)) => transport.preview_sample(&id),
                    Some(view::Selection::Pattern(id)) => transport.preview_pattern(&id),
                    _ => {}
                }
                if let Some(seconds) = screenshot::initial_play_at() {
                    transport.seek(seconds);
                    transport.play();
                }
                State::Ready(Box::new(Ready {
                    loaded: *loaded,
                    view,
                    transport,
                    stream_error_logged: false,
                }))
            }
            Ok(LoadOutcome::Failed { path, message }) => {
                self.errors.push(tf(
                    "status.could_not_open",
                    &[("file", &file_name(&path)), ("message", &message)],
                ));
                State::Failed { path, message }
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                let message = t("error.loader_stopped").to_string();
                self.errors.push(tf(
                    "status.could_not_open",
                    &[("file", &file_name(path)), ("message", &message)],
                ));
                State::Failed { path: path.clone(), message }
            }
        };
        self.state = next;
        if let Some(path) = opened {
            self.remember(&path);
        }
    }

    /// Tools > Export > WAV: asks where to save and writes the composition's
    /// full mix, then reports the result in a dialog.
    fn export_wav(&mut self) {
        let State::Ready(ready) = &self.state else { return };
        let Some(session) = &ready.loaded.session else { return };
        let mut dialog = rfd::FileDialog::new()
            .set_title(t("export.title"))
            .add_filter(t("export.filter"), &["wav"])
            .set_file_name(export_file_name(&ready.loaded.project.path));
        if let Some(dir) = dialog_dir(&self.config) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else { return };
        match write_export(session, &path) {
            // A success is reported in a dialog; a failure goes to the log.
            Ok(()) => {
                self.notice = tf("export.done", &[("path", &path.display().to_string())]);
                self.dialog = Some(Dialog::Notice);
            }
            Err(err) => {
                self.errors.push(tf("export.failed", &[("error", &err.to_string())]));
            }
        }
    }

    /// Adds a successfully opened composition to the history and saves it.
    fn remember(&mut self, path: &std::path::Path) {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.config.record_opened(&absolute.to_string_lossy());
        self.save_config();
    }

    fn save_config(&mut self) {
        self.config_dirty_since = None;
        if let Some(config_path) = &self.config_path {
            if let Err(e) = self.config.save(config_path) {
                eprintln!("could not save {}: {e}", config_path.display());
                self.errors.push(tf(
                    "error.config_save",
                    &[("path", &config_path.display().to_string()), ("error", &e.to_string())],
                ));
            }
        }
    }

    /// Keeps the divider positions of the open composition in the
    /// configuration (saved by `track_window`'s delayed write).
    fn track_dividers(&mut self) {
        let State::Ready(r) = &self.state else { return };
        let now = DividerState {
            tabs_width_percent: r.view.tabs_width_percent.round() as i32,
            arrangement_height_percent: r.view.arrangement_height_percent.round() as i32,
        };
        if now != self.config.dividers() {
            self.config.set_dividers(&now);
            self.config_dirty_since = Some(std::time::Instant::now());
        }
    }

    /// Keeps the window state in the configuration and saves it shortly
    /// after the last change (so dragging or resizing does not write on
    /// every frame).
    fn track_window(&mut self, ctx: &egui::Context) {
        const SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

        let (maximized, minimized, outer, inner) = ctx.input(|i| {
            let v = i.viewport();
            (v.maximized, v.minimized, v.outer_rect, v.inner_rect)
        });
        // Nothing to learn while minimized (positions are meaningless then)
        // or before the system has reported the window.
        if minimized == Some(true) {
            return;
        }
        if let Some(maximized) = maximized {
            let mut state = self.config.window();
            state.maximized = maximized;
            // Only a restored window's geometry is worth remembering.
            if !maximized {
                if let Some(inner) = inner {
                    state.width = (inner.width().round() as i32).max(200);
                    state.height = (inner.height().round() as i32).max(150);
                }
                if let Some(outer) = outer {
                    state.position = Some((outer.min.x.round() as i32, outer.min.y.round() as i32));
                }
            }
            if state != self.config.window() {
                self.config.set_window(&state);
                self.config_dirty_since = Some(std::time::Instant::now());
            }
        }
        if let Some(since) = self.config_dirty_since {
            if since.elapsed() >= SAVE_DELAY {
                self.save_config();
            } else {
                ctx.request_repaint_after(SAVE_DELAY);
            }
        }
    }

    fn window_title(&self) -> String {
        match &self.state {
            State::Ready(r) => format!(
                "{} - {APP_TITLE}",
                r.loaded.project.composition.metadata.title
            ),
            _ => APP_TITLE.to_string(),
        }
    }
}

/// What the status bar shows for a state.
fn status_of(state: &State) -> StatusLine<'_> {
    match state {
        State::Empty => StatusLine::Empty,
        State::Loading { path, .. } => StatusLine::Loading(borrowed_file_name(path)),
        State::Failed { path, .. } => StatusLine::Failed(borrowed_file_name(path)),
        State::Ready(r) => StatusLine::Ready(&r.loaded),
    }
}

/// The file name of `path` as a borrowed `&str` (lossy fallback to the whole
/// path), without allocating on every frame.
fn borrowed_file_name(path: &std::path::Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .or_else(|| path.to_str())
        .unwrap_or("?")
}

fn dropped_path(ctx: &egui::Context) -> Option<PathBuf> {
    ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()))
}

/// The folder file dialogs start in: the `path` setting, when it names a
/// folder that exists.
fn dialog_dir(config: &Config) -> Option<PathBuf> {
    config.files_path().map(PathBuf::from).filter(|p| p.is_dir())
}

fn pick_file(config: &Config) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(t("file.open_title"))
        .add_filter(t("file.open_filter"), &["bm1"]);
    if let Some(dir) = dialog_dir(config) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

/// The file name a composition's export starts with: its own name with a
/// `.wav` extension.
fn export_file_name(composition: &Path) -> String {
    let stem = composition
        .file_stem()
        .map_or_else(|| "export".to_string(), |s| s.to_string_lossy().into_owned());
    format!("{stem}.wav")
}

/// Renders nothing new: writes the composition's full mix (the same audio
/// `bm export --wav` writes) to `path`.
fn write_export(session: &bm_session::Session, path: &Path) -> Result<(), bm_wav::WavError> {
    bm_wav::write_wav(path, &session.master, bm_render::OUTPUT_SAMPLE_RATE)
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        i18n::set_language(screenshot::language_override().unwrap_or_else(|| self.config.language()));

        if screenshot::icon_mode() {
            // Developer aid: just the icon glyph, white on black, filling the window.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        egui_phosphor::regular::FILE_AUDIO,
                        egui::FontId::proportional(rect.height().min(rect.width()) * 0.9),
                        egui::Color32::WHITE,
                    );
                });
            if let Some(job) = &mut self.screenshot {
                job.tick(&ctx);
            }
            return;
        }
        if let Some(path) = dropped_path(&ctx) {
            self.open(&ctx, path);
        }
        self.poll_loading();
        if let Some(job) = &mut self.screenshot {
            job.tick(&ctx);
        }
        // The window's close button asks for the same confirmation as
        // File > Quit.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::ConfirmQuit);
        }
        if self.screenshot.is_some() && screenshot::keep_errors_empty() {
            self.errors.clear();
        }
        self.track_dividers();
        self.track_window(&ctx);

        // Space toggles play/pause; keep the engine in step with the view.
        let mut stream_error = false;
        if let State::Ready(r) = &mut self.state {
            if r.transport.has_stream_error() && !r.stream_error_logged {
                r.stream_error_logged = true;
                stream_error = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                r.transport.toggle_play();
            }
            r.transport.sync(&r.view);
            if r.transport.is_playing() || r.transport.any_preview_playing() {
                ctx.request_repaint();
            }
        }

        if stream_error {
            self.errors.push(t("error.stream"));
        }

        let mut actions = chrome::MenuActions::default();
        let recent: Vec<&str> = self.config.last_opened.iter().map(|e| e.value.as_str()).collect();
        let can_export = matches!(&self.state, State::Ready(r) if r.loaded.session.is_some());
        egui::Panel::top("menu_bar")
            .show(ui, |ui| actions = chrome::menu_bar(ui, &recent, can_export));

        // Bottom ribbons: the status bar is the lowest, the transport sits
        // right above it, directly under the arrangement.
        let errors = &self.errors;
        let mut open_errors = false;
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            open_errors = match &mut self.state {
                State::Ready(r) => {
                    let Ready { loaded, view, .. } = &mut **r;
                    let sliders = StatusSliders {
                        volume: &mut view.volume,
                        last_volume: &mut view.last_volume,
                        zoom: &mut view.step_width,
                    };
                    chrome::status_bar(ui, &StatusLine::Ready(loaded), Some(sliders), errors)
                }
                other => chrome::status_bar(ui, &status_of(other), None, errors),
            };
        });
        if open_errors {
            self.dialog = Some(Dialog::Errors);
        }
        if let State::Ready(r) = &mut self.state {
            egui::Panel::bottom("transport_bar").show(ui, |ui| {
                let Ready { view, transport, .. } = &mut **r;
                transport::show(ui, transport, view);
            });
        }

        egui::CentralPanel::default().show(ui, |ui| match &mut self.state {
            State::Empty => chrome::empty_state(ui),
            State::Loading { path, .. } => chrome::loading_state(ui, &file_name(path)),
            State::Failed { path, message } => {
                chrome::failed_state(ui, &file_name(path), message)
            }
            State::Ready(ready) => {
                let Ready { loaded, view, transport, .. } = &mut **ready;
                let total = ui.available_height();
                // As for the vertical divider, the size comes from the stored
                // percentage (see `panels::top_row`).
                let top = egui::Panel::top("info_panel")
                    .resizable(false)
                    .exact_size(total * (100.0 - view.arrangement_height_percent) / 100.0)
                    .show(ui, |ui| panels::top_row(ui, loaded, view, transport));
                let edge = top.response.rect;
                let delta = panels::splitter(
                    ui,
                    "top_splitter",
                    edge.left_bottom(),
                    edge.width(),
                    panels::Axis::Horizontal,
                );
                if total > 0.0 {
                    // Dragging down grows the top row, so C shrinks.
                    let range = view.divider_limits.arrangement_height_range();
                    view.arrangement_height_percent = panels::clamp_percent(
                        view.arrangement_height_percent - delta / total * 100.0,
                        total,
                        range,
                    );
                }
                let scopes = transport.track_scopes(widgets::scope_points(arrangement::LEFT_WIDTH));
                arrangement::show(
                    ui,
                    loaded,
                    view,
                    transport.playhead(),
                    transport.is_playing(),
                    &scopes,
                );
            }
        });

        if actions.dialog.is_some() {
            self.dialog = actions.dialog;
            if self.dialog == Some(Dialog::Settings) {
                self.settings_draft = Some(self.config.clone());
            }
        }
        let outcome = dialogs::show(
            &ctx,
            &mut self.dialog,
            &mut self.settings_draft,
            &self.notice,
            &mut self.errors,
        );
        if outcome.quit {
            self.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(edited) = outcome.settings {
            self.config.adopt_user_settings(&edited);
            self.save_config();
        }

        if let Some(path) = actions.open_path.take() {
            self.open(&ctx, path);
        }
        if actions.open {
            if let Some(path) = pick_file(&self.config) {
                self.open(&ctx, path);
            }
        }
        if actions.export_wav {
            self.export_wav();
        }
        if actions.quit {
            self.dialog = Some(Dialog::ConfirmQuit);
        }

        let title = self.window_title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn demo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../demos/songs/song1.bm1")
    }

    /// Polls until the background load finishes (or fails the test).
    fn wait_until_loaded(app: &mut PlayerApp) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while matches!(app.state, State::Loading { .. }) {
            assert!(Instant::now() < deadline, "load did not finish in time");
            std::thread::sleep(Duration::from_millis(10));
            app.poll_loading();
        }
    }

    #[test]
    fn starts_empty_and_titled_with_the_app_name() {
        let app = PlayerApp::new(&egui::Context::default(), None, None);
        assert!(matches!(app.state, State::Empty));
        assert_eq!(app.window_title(), "bit-music gui-player");
    }

    #[test]
    fn opening_the_demo_ends_up_ready_with_the_composition_title() {
        let mut app = PlayerApp::new(&egui::Context::default(), Some(demo()), None);
        assert!(matches!(app.state, State::Loading { .. }));
        wait_until_loaded(&mut app);
        assert!(matches!(app.state, State::Ready(_)));
        assert_eq!(app.window_title(), "Demo - bit-music gui-player");
        // A successful open goes into the history; a failed one does not.
        assert_eq!(app.config.last_opened.len(), 1);
        assert!(app.config.last_opened[0].key.ends_with("song1.bm1"));
    }

    #[test]
    fn a_loader_thread_that_dies_ends_up_failed_instead_of_loading_forever() {
        let mut app = PlayerApp::new(&egui::Context::default(), None, None);
        let (tx, rx) = std::sync::mpsc::channel::<LoadOutcome>();
        drop(tx); // the thread went away without answering
        app.state = State::Loading {
            path: "song.bm1".into(),
            rx,
        };
        app.poll_loading();
        assert!(matches!(&app.state, State::Failed { message, .. } if message.contains("unexpectedly")));
    }

    #[test]
    fn opening_a_missing_file_ends_up_failed() {
        let mut app = PlayerApp::new(&egui::Context::default(), Some("/no/such/file.bm1".into()), None);
        wait_until_loaded(&mut app);
        assert!(matches!(app.state, State::Failed { .. }));
        assert_eq!(app.window_title(), "bit-music gui-player");
        assert!(app.config.last_opened.is_empty());
    }

    #[test]
    fn the_export_writes_the_full_mix_that_bm_export_writes() {
        let mut app = PlayerApp::new(&egui::Context::default(), Some(demo()), None);
        wait_until_loaded(&mut app);
        let State::Ready(ready) = &app.state else { panic!("the demo should be open") };
        let session = ready.loaded.session.as_ref().expect("the demo has audio");

        let dir = std::env::temp_dir().join(format!("gui-player-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(export_file_name(&ready.loaded.project.path));
        assert_eq!(path.file_name().unwrap(), "song1.wav");

        write_export(session, &path).unwrap();
        let back = bm_wav::load_wav(&path).unwrap();
        assert_eq!(back.sample_rate, bm_render::OUTPUT_SAMPLE_RATE);
        assert_eq!(back.data.len(), session.master.len());
        assert!(back.data.iter().any(|v| *v != 0.0));
    }

    #[test]
    fn dialogs_start_in_the_configured_folder_only_if_it_exists() {
        let mut config = Config::default().with_defaults();
        // The default is a Windows path of the author's machine: absent here.
        if !Path::new(&config.files_path().unwrap()).is_dir() {
            assert_eq!(dialog_dir(&config), None);
        }
        config.set(crate::config::PATH, serde_json::Value::from(std::env::temp_dir().to_string_lossy().to_string()));
        assert_eq!(dialog_dir(&config), Some(std::env::temp_dir()));
        config.set(crate::config::PATH, serde_json::Value::from("  "));
        assert_eq!(dialog_dir(&config), None);
    }

    #[test]
    fn the_language_setting_changes_the_interface_language() {
        let mut app = PlayerApp::new(&egui::Context::default(), None, None);
        assert_eq!(i18n::language(), i18n::Lang::English);
        app.config.set(crate::config::LANG, serde_json::Value::from("SPANISH"));
        i18n::set_language(app.config.language());
        assert_eq!(t("menu.file"), "Archivo");
        i18n::set_language(i18n::Lang::English);
    }
}
