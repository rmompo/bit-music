//! Application state and the main UI loop.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;

use crate::dialogs::{self, Dialog};
use crate::chrome::{self, StatusLine, StatusSliders};
use crate::loader::{self, file_name, LoadOutcome, Loaded};
use crate::panels;
use crate::screenshot::{self, ScreenshotJob};
use crate::transport::{self, Transport};
use crate::view::ViewState;
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
}

pub struct PlayerApp {
    state: State,
    last_title: String,
    /// The modal dialog currently open, if any.
    dialog: Option<Dialog>,
    /// Developer aid, only set through `BM_GUI_SCREENSHOT`.
    screenshot: Option<ScreenshotJob>,
}

const APP_TITLE: &str = dialogs::PRODUCT;

impl PlayerApp {
    /// `initial` is a composition to open at startup (e.g. from the command line).
    pub fn new(ctx: &egui::Context, initial: Option<PathBuf>) -> Self {
        chrome::install_icon_font(ctx);
        let mut app = Self {
            state: State::Empty,
            last_title: String::new(),
            dialog: screenshot::initial_dialog(),
            screenshot: ScreenshotJob::from_env(),
        };
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
        let next = match rx.try_recv() {
            Ok(LoadOutcome::Loaded(loaded)) => {
                let mut view = ViewState::new(&loaded);
                if let Some((selection, tab)) = screenshot::initial_selection() {
                    view.selection = selection;
                    view.tab = tab;
                }
                let transport = Transport::new(&loaded);
                if let Some(seconds) = screenshot::initial_play_at() {
                    transport.seek(seconds);
                    transport.play();
                }
                State::Ready(Box::new(Ready {
                    loaded: *loaded,
                    view,
                    transport,
                }))
            }
            Ok(LoadOutcome::Failed { path, message }) => State::Failed { path, message },
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => State::Failed {
                path: path.clone(),
                message: "loading stopped unexpectedly (internal error)".to_string(),
            },
        };
        self.state = next;
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
        State::Failed { path, message } => StatusLine::Failed {
            file: borrowed_file_name(path),
            message,
        },
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

fn pick_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open a bit-music composition")
        .add_filter("bit-music composition", &["bm1"])
        .pick_file()
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if let Some(path) = dropped_path(&ctx) {
            self.open(&ctx, path);
        }
        self.poll_loading();
        if let Some(job) = &mut self.screenshot {
            job.tick(&ctx);
        }

        // Space toggles play/pause; keep the engine in step with the view.
        if let State::Ready(r) = &mut self.state {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                r.transport.toggle_play();
            }
            r.transport.sync(&r.view);
            if r.transport.is_playing() {
                ctx.request_repaint();
            }
        }

        let mut actions = chrome::MenuActions::default();
        egui::Panel::top("menu_bar").show(ui, |ui| actions = chrome::menu_bar(ui));

        // Bottom ribbons: the status bar is the lowest, the transport sits
        // right above it, directly under the arrangement.
        egui::Panel::bottom("status_bar").show(ui, |ui| match &mut self.state {
            State::Ready(r) => {
                let Ready { loaded, view, .. } = &mut **r;
                let sliders = StatusSliders { volume: &mut view.volume, zoom: &mut view.step_width };
                chrome::status_bar(ui, &StatusLine::Ready(loaded), Some(sliders));
            }
            other => chrome::status_bar(ui, &status_of(other), None),
        });
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
                let Ready { loaded, view, transport } = &mut **ready;
                egui::Panel::top("info_panel")
                    .resizable(true)
                    .default_size(330.0)
                    .show(ui, |ui| panels::top_row(ui, loaded, view, transport));
                arrangement::show(ui, loaded, view, transport.playhead(), transport.is_playing());
            }
        });

        if actions.dialog.is_some() {
            self.dialog = actions.dialog;
        }
        dialogs::show(&ctx, &mut self.dialog);

        if actions.open {
            if let Some(path) = pick_file() {
                self.open(&ctx, path);
            }
        }
        if actions.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../player/demos/songs/song1.bm1")
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
        let app = PlayerApp::new(&egui::Context::default(), None);
        assert!(matches!(app.state, State::Empty));
        assert_eq!(app.window_title(), "bit-music gui-player");
    }

    #[test]
    fn opening_the_demo_ends_up_ready_with_the_composition_title() {
        let mut app = PlayerApp::new(&egui::Context::default(), Some(demo()));
        assert!(matches!(app.state, State::Loading { .. }));
        wait_until_loaded(&mut app);
        assert!(matches!(app.state, State::Ready(_)));
        assert_eq!(app.window_title(), "Demo - bit-music gui-player");
    }

    #[test]
    fn a_loader_thread_that_dies_ends_up_failed_instead_of_loading_forever() {
        let mut app = PlayerApp::new(&egui::Context::default(), None);
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
        let mut app = PlayerApp::new(&egui::Context::default(), Some("/no/such/file.bm1".into()));
        wait_until_loaded(&mut app);
        assert!(matches!(app.state, State::Failed { .. }));
        assert_eq!(app.window_title(), "bit-music gui-player");
    }
}
