# bit-music shared libraries

Everything that can be common to more than one component (`bm` CLI,
`gui-player`, `gui-editor`) lives in a library with a single responsibility,
instead of inside an application. Applications only orchestrate and present.

Status: **implemented**. The libraries live in `libs/` under a Cargo
workspace at the repository root; the linker config for Windows
cross-compilation is the root `.cargo/config.toml`; build scripts are in
`scripts/`.

```
bit-music/
  Cargo.toml            workspace (members: libs/*, tools/*, player, gui-player)
  .cargo/config.toml    mingw linker for x86_64-pc-windows-gnu
  scripts/              build-windows.sh / build-linux.sh / build-all.sh [package]
  libs/                 bm-dsp bm-format bm-wav bm-timeline bm-project
                        bm-render bm-playback bm-session
  tools/                development tools (gen-demo-samples)
  player/               the `bm` CLI
  gui-player/           the graphical player (egui/eframe)
```

## The libraries

| Crate | Responsibility | Originally in `player/src` |
|---|---|---|
| `bm-dsp` | Numeric audio primitives: `AudioBuffer`, resampling, downmix, normalize, mix-into, semitones→ratio. Pure, no I/O, real-time safe | `audio` (resample/downmix), `mix` (mix_into/normalize) |
| `bm-format` | The `.bm1` contract: serde model, note notation, validation rules, supported format versions, default resolution, parse/serialize from/to `&str`. Pure, no file or audio I/O | `model`, `note`, `validate` |
| `bm-wav` | Read, write and header-check `.wav` files (returns `bm-dsp::AudioBuffer`) | `audio` (load/write/check) |
| `bm-timeline` | Arrangement → per-track timeline (grid/column model, looping, silence padding) and `seconds_per_step`. Pure | `resolve` |
| `bm-project` | Everything touching the file system for a composition: load a `.bm1`, resolve sample paths relative to it, report which samples are present/valid. Returns structured reports, never prints | `loader`, sample checks from `commands` |
| `bm-render` | Timeline + decoded samples → one buffer per track and the master mix; pitch-shift per step | `mix`, `pitch` |
| `bm-playback` | Audio device output (`cpal`): non-blocking engine with play/pause/stop/seek/loop, per-track mute, master volume, sample previews, position exposed atomically | `playback` |
| `bm-session` | Thin facade "open a project and get everything ready to play/export". No logic of its own | `load_and_mix` in `commands` |

What stays in each application (not shared): CLI help rendering, ANSI
colors, keyboard listener, subcommand dispatch and console output (`bm`);
windows, widgets and drawing (`gui-*`).

## Dependency layers

Dependencies only point downward; nothing depends on an application.

```
Layer 0  bm-dsp
Layer 1  bm-format        bm-wav (-> bm-dsp)
Layer 2  bm-timeline (-> bm-format)
         bm-project  (-> bm-format, bm-wav)
         bm-playback (-> bm-dsp, cpal)
Layer 3  bm-render   (-> bm-format, bm-timeline, bm-dsp)
Layer 4  bm-session  (-> bm-project, bm-wav, bm-timeline, bm-render, bm-dsp)
Apps     bm, gui-player, gui-editor
Tools    gen-demo-samples (-> bm-wav)
```

## Who uses what

| Consumer | Libraries |
|---|---|
| `bm` (CLI) | `bm-format`, `bm-project`, `bm-session`, `bm-render`, `bm-wav`, `bm-playback` |
| `gui-player` | `bm-format`, `bm-project`, `bm-session`, `bm-timeline`, `bm-playback` |
| `tools/gen-demo-samples` | `bm-wav` |

## Rules

- **Single responsibility**: a crate that needs "and" to describe it should be split.
- **DRY across components**: logic used by two components moves into a library, never copied.
- **Only `bm-playback` depends on `cpal`.** Every other crate builds anywhere without system audio libraries, which keeps cross-compilation and tests simple.
- **Libraries never print or read the terminal.** They return values and structured errors (`thiserror` per crate); applications decide how to show them.
- **No UI or framework types in libraries**, so the GUI framework can be swapped without touching the core.

## Evolution beyond the original prototype

Beyond moving code, the split added what a GUI and an editor need:

- `bm-timeline` also returns the grid `columns` and per-track `clips`
  (pattern, column, start step, length, whether it is a loop repetition), so
  a UI can draw blocks without re-implementing the grid model.
- `bm-render` produces one buffer per track (`render_tracks`) and mixes them
  with an optional mute list (`mix_tracks`).
- `bm-playback` is non-blocking and real-time safe: position is an atomic
  counter, seeks are requests consumed by the callback, mute flags are
  atomics, and there are no locks or allocations in the audio callback. It
  also has a master volume and one-shot **previews** (`Engine::with_previews`
  + `play_preview(index)`): short sounds that mix on top of the transport,
  even when paused, for auditioning a sample or a pattern.
- `bm-render` can also render a single pattern on its own (`render_pattern`). Its
  mixing logic is separated from `cpal` and unit-tested.
- `bm-format` can serialize (`to_json`) as well as parse.

Not yet done: solo per track, envelope/note duration, stereo.

## Versioning

Each library carries its own version in its `Cargo.toml` (not inherited from
the workspace), bumped when its public API changes: `bm-playback` is at
0.3.0 (master volume, previews and asking whether one is sounding),
`bm-render` at 0.2.0 (`render_pattern`), `bm-project` at 0.2.0 (declared sample
files) and the rest at 0.1.0. The GUI's *Tools > Libraries* window shows them.

## Licensing

Every crate inherits `license = "MIT OR Apache-2.0"` (and the repository URL)
from the workspace manifest. `THIRD_PARTY_LICENSES.md` lists the notices of the
third-party crates in the Windows and Linux binaries, and is regenerated with
`scripts/gen-third-party-licenses.py`.
