// On Windows release builds, don't open a console window behind the GUI.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod arrangement;
mod chrome;
mod config;
mod dialogs;
mod fatal;
mod fmt;
mod grid;
mod loader;
mod palette;
mod panels;
mod screenshot;
mod transport;
mod view;
mod widgets;

use std::path::PathBuf;

use eframe::egui;

fn main() {
    fatal::install_panic_hook();
    if let Err(err) = run() {
        fatal::report(&fatal::startup_error_text(&err.to_string()));
        std::process::exit(1);
    }
}

fn run() -> eframe::Result {
    // Optional first argument: a .bm1 to open at startup.
    let initial = std::env::args_os().nth(1).map(PathBuf::from);

    // Open the window the way it was left (see gui-player.json).
    let config_path = config::Config::default_path();
    let window = config_path
        .as_deref()
        .map(|p| config::Config::load_or_create(p).0.window())
        .unwrap_or_else(|| config::Config::default().with_defaults().window());
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("bit-music gui-player")
        .with_inner_size([window.width as f32, window.height as f32])
        .with_min_inner_size([640.0, 420.0])
        .with_maximized(window.maximized);
    if let Some((x, y)) = window.position {
        viewport = viewport.with_position([x as f32, y as f32]);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };

    eframe::run_native(
        "bit-music gui-player",
        options,
        Box::new(move |cc| Ok(Box::new(app::PlayerApp::new(&cc.egui_ctx, initial, config_path)))),
    )
}
