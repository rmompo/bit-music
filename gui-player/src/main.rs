// On Windows release builds, don't open a console window behind the GUI.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod arrangement;
mod chrome;
mod fatal;
mod fmt;
mod grid;
mod loader;
mod palette;
mod panels;
mod screenshot;
mod transport;
mod view;

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

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("bit-music player")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        "bit-music player",
        options,
        Box::new(move |cc| Ok(Box::new(app::PlayerApp::new(&cc.egui_ctx, initial)))),
    )
}
