# player — architecture

Command-line player, built as the `bm` executable: `bm play song.bm1`,
plus validation subcommands (`bm check-integrity`, `bm check-samples`,
`bm check`), `bm export --wav`, `bm version`, `bm help`, and the
placeholder `bm magik`.

## Decisions made

- **Language**: Rust. Chosen over Java because of: (a) timing-critical
  multi-channel mixing with no GC pauses, (b) a real native executable with
  no intermediate runtime (`cargo build --release`), versus the extra
  complexity of GraalVM Native Image / `jpackage` in Java.
- **Compilation target**: cross-compiled from WSL2/Linux to
  `x86_64-pc-windows-gnu` (mingw-w64) — the final execution target is
  Windows. A native Linux build (`x86_64-unknown-linux-gnu`, ALSA backend)
  is also supported for development/testing inside WSL2. Build scripts live
  in the repository's `scripts/` (`scripts/build-windows.sh [package]`, ...)
  and the mingw linker config in the root `.cargo/config.toml`, since `bm`
  is now one member of a Cargo workspace.
- **Audio strategy**: *pre-render*, not real-time synthesis. The whole
  composition is resolved and rendered offline to one PCM buffer per track
  (`bm-render`), and **then** the `bm-playback` engine mixes those buffers
  in its audio callback with only atomic state (no locks, no allocation).
  This keeps any computation pause (parsing, resampling, summing) from
  affecting sync between channels: all the heavy work happens before
  anything starts playing, and playback itself is just adding pre-computed
  samples.
- **Pitch-shift method**: classic sampler/tracker resampling (playback
  speed change) rather than time-stretching — simple, cheap, and the
  expected behavior for this kind of format.
- **CLI**: `clap` (derive), subcommand-based (`play`, `check-integrity`,
  `check-samples`, `check`, `export`, `version`, `help`, `magik`).
- **No short flags, and no `--help`/`--version` flags.** Options are long-only
  (`--non-stop`, `--wav`), and `version` and `help` exist only as
  subcommands (`bm version`, `bm help`), so there is a single way to ask for
  each. `-h`, `-V`, `-v` and `--help`/`--version` are rejected at the top level
  and by every subcommand except `magik`, which deliberately accepts arbitrary
  trailing arguments because its switches are not defined yet.
- **`check-integrity` vs `check-samples`**: the first validates the structure
  and the id references (through `bm_project::load`, which parses and
  validates with `bm-format`) without touching the audio files; the second
  additionally checks that each sample file exists and is a well-formed
  `.wav`, reading only its header (`bm_project::check_samples`); `check` runs
  both.
- **Format version guard**: `bm version` prints the tool version and the
  supported `.bm1` format versions, and loading rejects a composition whose
  `metadata.version` is not supported.
- **`bm export`**: reuses the same pipeline as `play` (`bm_session::open`),
  writing the mixed buffer to a `.wav` with the same base name as the input
  via `bm_wav::write_wav`
  (16-bit PCM). Only `--wav` is implemented; MP3 was deliberately deferred
  — a real MP3 encoder either means embedding LAME via `mp3lame-sys`
  (autotools-based build, cross-compilation risk not yet evaluated) or
  shelling out to an external `ffmpeg`/`lame`, which would break the
  single-dependency-free-executable design goal. `--wav` without a
  `--mp3` counterpart is intentional for now; `bm export` without `--wav`
  fails with a clear error instead of silently doing nothing.
- **Help text**: `bm help` renders a structured JSON file (`bm.hlp`, see
  `help.rs`) instead of hardcoded strings, so the help content can be
  edited without recompiling. Looked up next to the running executable
  first, falling back to a copy embedded into the binary at build time
  (`include_str!`) if it's missing or invalid.
- **`bm play --non-stop`**: puts the `bm-playback` engine in loop mode
  (seamless wrap-around, no stream re-creation) and waits until interrupted.
  A background thread (`keyboard.rs`, `crossterm`) puts the terminal in raw
  mode and watches for Escape or Ctrl+C (raw mode intercepts the normal
  SIGINT delivery, so Ctrl+C is detected as a regular key event instead),
  setting a shared `Arc<AtomicBool>` that the wait loop in `commands::play`
  polls.
- **`bm magik`**: placeholder for a future command that automatically
  edits/improves a composition. It will **not** modify the input file in
  place — it writes the result to a new file. Switches/behavior still
  undefined; currently just prints "to be implemented".
- **Dependencies** (CLI only): `clap`, `crossterm` (raw-mode keyboard input
  for `--non-stop`), `serde`/`serde_json` (help file). Everything else comes
  from the shared `bm-*` libraries.

## Running the Windows build

The Windows executable is not code-signed, so Windows 11 Smart App Control
can block it. On a development machine, Windows Developer Mode lets it run;
see `../../specs/ci-and-signing.md`, which also documents the CI workflow that
used to build it and how to get signed binaries. The demo composition and its
synthesized samples live in `demos/`.

## Modules

The player is a thin application over the shared libraries described in
`../../specs/libraries.md` (format, project loading, timeline, rendering,
playback, ...). What lives in `player/src` is only what is specific to the
command line:

- `commands.rs` — each CLI subcommand: chains the libraries and decides how
  to print results.
- `help.rs` — loads and renders `bm.hlp` (external, falling back to the
  build-time embedded copy), with ANSI colors and terminal-width wrapping.
- `keyboard.rs` — background raw-mode keyboard listener used by
  `bm play --non-stop` to detect Escape/Ctrl+C.
- `main.rs` — CLI definition (`clap`) and dispatch.

## Known limitations (candidates for a future iteration)

- **No note envelope/duration**: each sample plays in full once triggered
  (no cutoff on the next step, no fade-out), which can cause heavy overlap
  with long, sustained samples.
- **Mono only**: everything is downmixed to mono; there is no stereo
  panning.
- **Simple normalization**: one fixed gain, computed from the peak of the
  full mix, keeps it from clipping. Heavy overlap therefore lowers the overall
  volume (and muting a track makes the rest quieter) instead of being handled
  with something more elaborate such as compression.
