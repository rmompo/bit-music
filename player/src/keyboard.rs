//! Background keyboard listener used to interrupt `bm play --non-stop`.
//!
//! Puts the terminal into raw mode and watches for Escape or Ctrl+C,
//! setting a shared flag when either is pressed. Raw mode intercepts the
//! normal SIGINT delivery for Ctrl+C, so it's detected here as a regular
//! key event instead.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// Spawns a background thread that watches stdin for Escape or Ctrl+C and
/// sets `stop` when either is seen. Returns immediately.
///
/// If the terminal can't be put into raw mode (e.g. stdin isn't a real
/// terminal — piped input, some CI environments), this silently does
/// nothing: `--non-stop` playback then only stops via a process-level
/// Ctrl+C (SIGINT), which still works normally since raw mode was never
/// enabled.
pub fn watch_for_interrupt(stop: Arc<AtomicBool>) {
    if enable_raw_mode().is_err() {
        return;
    }

    std::thread::spawn(move || {
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => {
                    if let Ok(Event::Key(key)) = event::read() {
                        let is_escape = key.code == KeyCode::Esc;
                        let is_ctrl_c = key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        if is_escape || is_ctrl_c {
                            stop.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                Ok(false) => {}
                Err(_) => break,
            }
        }
        let _ = disable_raw_mode();
    });
}
