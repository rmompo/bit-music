# gui-player — layout and MVP scope

Desktop application (Windows, Ubuntu and similar) that opens a `.bm1`
composition, shows its contents and plays it. The current `player` prototype
evolves into `gui-player`, which later evolves into `gui-editor` (the same
application with editing on top).

Status: **layout agreed; framework agreed (egui/eframe)**. The shared
libraries it builds on are implemented (see `../../specs/libraries.md`).

## Implementation status

- **Done:**
  - Window with menu bar (`File > Open…`, `Quit`); opening a `.bm1` by dialog, drag and drop, or as the first command-line argument; loading on a background thread; status bar; empty, loading and failed screens. A composition whose samples are missing still opens (with warnings and no audio).
  - **A** metadata.
  - **B** tabs *Samples* / *Patterns*, each a list of names with a color chip (the sample's color); clicking selects.
  - **C** properties (vertical scroll) of the selected sample or pattern. A pattern shows its facts, the **step grid** (piano-roll style: one row per distinct pitch, one column per step, beats shaded and numbered) and where it is used; a sample shows status, root note, length, sample rate, file path and the patterns that use it.
  - **D** arrangement: one row per track with a **mute button (M)** and name, and a step grid where every pattern is a filled block colored by its sample with its notes drawn inside, loop repetitions dimmed, column shading, beat lines, a pinned ruler (column numbers and start times) and pinned track column, shared 2D scroll, zoom slider, and click-to-select a block (which selects the pattern in C). Selecting a pattern highlights its blocks.
  - **Transport ribbon**, directly under the arrangement: Play, Pause, Stop, Loop, `elapsed / total` and a position slider (dragging seeks). Space toggles play/pause. It drives the `bm-playback` engine; the mute buttons and Loop are pushed to the engine every frame. A red **playback cursor** (line plus a marker on the ruler) crosses the arrangement, and the view scrolls to keep it visible while playing. Without an audio device (or without audio because samples are missing) the controls are disabled and the reason is shown; the app still works as a viewer.
  - **Status bar**: file, integrity, sample count on the left; the arrangement **zoom** slider on the right.
- **Startup errors are never silent.** The Windows release build has no console, so a failure to start (no usable graphics, window creation error) and any panic on the main thread are shown in a native error dialog (and printed to stderr). If the loading thread dies, the app shows a failure instead of "Loading…" forever.
- **Note on time:** the total shown by the transport is the length of the rendered audio, which can exceed the arrangement (the demo's arrangement is 2.5 s, but its piano note keeps ringing and the audio lasts about 4.2 s). The cursor is only drawn while it is inside the arrangement.
- **Next:** *File > Export WAV…* and the remaining status details; click-on-ruler seeking; a follow toggle.
- Tests: pure model tests (pattern grid), headless egui runs of every panel and of the arrangement, and load/state tests.

### Reviewing the UI without a screen

Two environment variables, meant for development only, let the UI be checked or documented without a person looking at the window:

- `BM_GUI_SCREENSHOT=<file.ppm>`: after a few frames, save a screenshot of the window and quit.
- `BM_GUI_SELECT=sample:<id>` or `pattern:<id>`: start with that element selected (so C has something to show).
- `BM_GUI_PLAY_AT=<seconds>`: start playing from that position as soon as the composition is open (needs an audio device), to capture the transport and cursor in action.

To capture the Windows build from WSL, forward the variables with `WSLENV`, for example `WSLENV="BM_GUI_SCREENSHOT/p:BM_GUI_PLAY_AT:BM_GUI_SELECT"` (the `/p` converts the path to a Windows path).

Observation: under WSLg the Wayland connection failed about every second launch (`Io error: Broken pipe`); the same binary was reliable through X11 (`env -u WAYLAND_DISPLAY`).

## Layout

```
+----------------------------------------------------------------------+
| File:  Open...   Quit                                                |   menu bar
+----------------------+----------------------+------------------------+
| A Metadata           | B [Samples][Patterns]| C Properties           |   top panel
| (scroll V)           |   list of names      |   detail of the item   |   (resizable)
|                      |   (scroll V)         |   selected in B or D   |
|                      |                      |   (scroll V)           |
+-----+----------------------------------------------------------------+
|     | ruler:  1 00:00.0 | 2 00:01.5 | ...                            |
| [M] | kick   [kickA     ][kickA     ]                                |   D Arrangement
| [M] | snare  [snareA    ][snareA    ]                                |   (scroll V/H
| [M] | hihat  [hihatA    ][hihatA    ]                                |    shared)
| [M] | epiano [epianoA   ][epianoA (loop)]                            |
| [M] | sax    [saxA       ]                                           |
+----------------------------------------------------------------------+
| [Play] [Pause] [Stop] [Loop]   00:03.2 / 00:08.6   |=====o----------|   transport
+----------------------------------------------------------------------+
| song1.bm1 - integrity OK - 5/5 samples                Zoom [--o----] |   status bar
+----------------------------------------------------------------------+
```

The arrangement has no title of its own, and its zoom slider lives at the right of the status bar.

## Areas

- **Menu bar**: `File` with *Open…* and *Quit* (*Export WAV…* is planned).
- **1 Metadata**: title, bpm, stepsPerBeat, format version, and the `others` key/value pairs. Vertical scroll.
- **2 Samples**: list of samples (id, file, root note/octave). Vertical scroll.
- **3 Patterns**: list of patterns (id, sample, length in steps). Vertical scroll.
- **4 Arrangement**: one row per track inside a single shared 2D scroll area (vertical and horizontal), so all tracks scroll together.
  - **Track row** = `[M]` + `[render]`. `[M]` is the mute toggle and stays fixed while the render scrolls horizontally. The render shows the track's patterns.
  - The column **ruler** stays fixed at the top of the arrangement.
- **Transport**, directly below the arrangement: play, pause, stop, loop, elapsed/total time and a position (seek) bar over the whole song.
- **Status bar**: file name, integrity result, sample availability summary.

## MVP scope

- Open a `.bm1` and show metadata, samples, patterns and the arrangement's tracks.
- Play, pause, stop, loop and seek, with a playback cursor moving across the arrangement.
- Mute per track.
- Export to WAV (same result as `bm export --wav`).
- Report integrity errors and missing/invalid samples with the same criteria as `bm check`.

Out of scope: editing compositions (that is `gui-editor`).

## Proposed refinements (not yet confirmed)

- Pattern blocks with width proportional to their steps, colored by sample (the samples list acts as the legend); loop repetitions dimmed; `null` gaps shown as empty space.
- Horizontal zoom.
- Cross-highlighting: selecting a pattern highlights where it is used in the arrangement, and vice versa; selecting a sample highlights its patterns.
- Missing/invalid sample warning marker in the samples list.
- Empty state when no file is open ("open or drop a .bm1").
- Collapsible top panel to give the arrangement full height.
- Solo per track (cheap once per-track buffers exist).

## After the MVP

- Waveform of the mix.
- Automatic reload when the `.bm1` changes on disk (fits the future editor workflow).

## Framework and platform notes

- GUI framework: **egui/eframe**. A spike (a minimal eframe window with `cpal` linked) built and ran on Windows (mingw cross-compilation from WSL) and on Linux (Wayland and X11), with about 0.6-1.4 ms of UI CPU per frame for 16,000 unculled rectangles. Electron was analyzed and not chosen for the player; the library split keeps the door open to a different front end later.
- Windows build should use the `windows` subsystem so no console window appears behind the GUI.
- `File > Open…` uses the native dialog through `rfd` (Win32 dialog on Windows; on Linux it goes through the desktop portal, so an `xdg-desktop-portal` service must be running).
- **Build gotcha (Windows target):** `wgpu-hal` and `gpu-allocator` must use the same `windows` crate version. `cpal 0.15` pins `windows 0.54`, and cargo may reuse it for `gpu-allocator` (which accepts `>=0.53, <=0.62`), breaking `wgpu-hal`'s DX12 code with "multiple versions of crate `windows`". `Cargo.lock` therefore points `gpu-allocator` to `windows 0.62.2`; if a `cargo update` reverts it, re-point that one entry (or upgrade `cpal`).
- Linux runtime needs the usual desktop libraries (Wayland/X11, Vulkan or GL); no development packages are needed to build.
