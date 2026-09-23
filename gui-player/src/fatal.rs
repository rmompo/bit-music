//! Reporting fatal errors where the user can actually see them.
//!
//! On Windows release builds the app has no console (`windows_subsystem =
//! "windows"`), so an error at startup — no usable graphics, a window that
//! can't be created, a panic — would otherwise make the program vanish
//! without a word. Here such errors also go to a native message box.

use std::panic::PanicHookInfo;

const TITLE: &str = "bit-music gui-player";

/// The text of the dialog shown when the app can't start.
pub fn startup_error_text(error: &str) -> String {
    format!("bit-music gui-player could not start.\n\n{error}")
}

/// Prints `message` to stderr and shows it in a native error dialog.
pub fn report(message: &str) {
    eprintln!("{message}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(TITLE)
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Makes a panic on the main thread show an error dialog (after the normal
/// panic message). Panics on other threads only get the normal message; the
/// code that waits on those threads reports the failure itself.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        if std::thread::current().name() == Some("main") {
            report(&panic_text(info));
        }
    }));
}

fn panic_text(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".to_string());
    match info.location() {
        Some(loc) => format!(
            "bit-music gui-player crashed.\n\n{message}\n\n({}:{})",
            loc.file(),
            loc.line()
        ),
        None => format!("bit-music gui-player crashed.\n\n{message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_text_includes_the_underlying_error() {
        let text = startup_error_text("no suitable graphics adapter");
        assert!(text.starts_with("bit-music gui-player could not start."));
        assert!(text.contains("no suitable graphics adapter"));
    }
}
