//! Implementations for the `bm` CLI subcommands: `play`, `check-integrity`,
//! `check-samples`, `check`, and `export`.
//!
//! The heavy lifting lives in the `bm-*` libraries; this module only chains
//! them and decides how to present the results on the console.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bm_playback::Engine;
use bm_render::OUTPUT_SAMPLE_RATE;
use bm_session::Session;

use crate::keyboard;

/// Loads and validates the structure of a `.bm1` composition (JSON schema,
/// note formats, id references, etc). Does not touch the referenced sample
/// files on disk — see `check_samples` for that.
pub fn check_integrity(file: &Path) -> bool {
    match bm_project::load(file) {
        Ok(project) => {
            let c = &project.composition;
            println!(
                "OK: '{}' (format version {}) is structurally valid ({} samples, {} patterns, {} tracks)",
                c.metadata.title,
                c.metadata.version,
                c.samples.len(),
                c.patterns.len(),
                c.arrangement.tracks.len(),
            );
            true
        }
        Err(err) => {
            eprintln!("FAIL: {err}");
            false
        }
    }
}

/// Loads a `.bm1` composition and checks that every referenced sample file
/// is present and a well-formed `.wav` (paths are already resolved by the
/// loader relative to the composition file).
pub fn check_samples(file: &Path) -> bool {
    let project = match bm_project::load(file) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("FAIL: could not load composition: {err}");
            return false;
        }
    };

    let mut all_ok = true;
    for report in bm_project::check_samples(&project.composition) {
        match report.outcome {
            Ok(()) => println!("OK: sample '{}' -> {}", report.sample_id, report.file),
            Err(err) => {
                eprintln!("FAIL: sample '{}' -> {}: {err}", report.sample_id, report.file);
                all_ok = false;
            }
        }
    }

    all_ok
}

/// Opens the composition (load, resolve, decode, render) and prints a
/// summary of what was built. Returns `None` after printing an error.
fn open_and_report(file: &Path) -> Option<Session> {
    let session = match bm_session::open(file) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("Error loading '{}': {err}", file.display());
            return None;
        }
    };

    let c = &session.project.composition;
    println!(
        "'{}' (format version {}) loaded: {} samples, {} patterns, {} tracks (bpm={})",
        c.metadata.title,
        c.metadata.version,
        c.samples.len(),
        c.patterns.len(),
        c.arrangement.tracks.len(),
        c.metadata.bpm,
    );

    println!(
        "Arrangement resolved: {} total steps ({:.3}s/step, {:.2}s total)",
        session.timeline.total_steps,
        session.seconds_per_step,
        session.timeline.total_steps as f64 * session.seconds_per_step,
    );
    for track in &session.timeline.tracks {
        let playing = track.steps.iter().filter(|s| s.is_some()).count();
        println!(
            "  track '{}': {} steps, {} playing, {} silent",
            track.id,
            track.steps.len(),
            playing,
            track.steps.len() - playing,
        );
    }

    for sample in &c.samples {
        if let Some(audio) = session.samples.get(&sample.id) {
            println!(
                "  sample '{}': {} frames @ {} Hz ({:.2}s)",
                sample.id,
                audio.data.len(),
                audio.sample_rate,
                audio.duration_seconds(),
            );
        }
    }

    println!(
        "Mixed buffer: {} frames @ {} Hz ({:.2}s)",
        session.master.len(),
        OUTPUT_SAMPLE_RATE,
        session.master_duration_seconds(),
    );

    Some(session)
}

/// Loads, renders, and plays a composition end to end.
///
/// If `non_stop` is set, loops it continuously until the user interrupts
/// with Ctrl+C or Escape.
pub fn play(file: &Path, non_stop: bool) -> bool {
    let Some(session) = open_and_report(file) else {
        return false;
    };

    let engine = match Engine::new(session.tracks.iter().map(|t| &t.audio)) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Playback error: {err}");
            return false;
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    engine.set_looping(non_stop);
    if non_stop {
        println!("Playing (--non-stop, press Ctrl+C or Escape to stop)...");
        keyboard::watch_for_interrupt(Arc::clone(&stop));
    } else {
        println!("Playing...");
    }
    engine.play();

    while !stop.load(Ordering::Relaxed) && !engine.has_finished() && !engine.has_stream_error() {
        std::thread::sleep(Duration::from_millis(20));
    }

    if engine.has_stream_error() {
        eprintln!("Playback error: the audio stream reported an error");
        return false;
    }

    if engine.has_finished() {
        // Let the device drain its output buffer before closing the stream.
        std::thread::sleep(Duration::from_millis(200));
        println!("Playback finished.");
    } else {
        println!("Stopped.");
    }
    true
}

/// Loads and renders a composition, then exports it to a `.wav` file with
/// the same base name as the input, in the same directory (e.g.
/// `songs/song1.bm1` -> `songs/song1.wav`).
///
/// `wav` must be `true` (the only export format implemented so far) —
/// `bm export` fails clearly otherwise instead of silently doing nothing.
pub fn export(file: &Path, wav: bool) -> bool {
    if !wav {
        eprintln!("Error: specify --wav (the only export format implemented so far)");
        return false;
    }

    let Some(session) = open_and_report(file) else {
        return false;
    };

    let output_path = file.with_extension("wav");
    match bm_wav::write_wav(&output_path, &session.master, OUTPUT_SAMPLE_RATE) {
        Ok(()) => {
            println!("Exported to '{}'", output_path.display());
            true
        }
        Err(err) => {
            eprintln!("Export error: {err}");
            false
        }
    }
}
