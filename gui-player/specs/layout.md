# gui-player — layout and MVP scope

Desktop application (Windows, Ubuntu and similar) that opens a `.bm1`
composition, shows its contents and plays it. It is built on the same
shared libraries as the `bm` command-line player, and it is meant to evolve
into `gui-editor` (the same application with editing on top).

Status: **layout agreed; framework agreed (egui/eframe)**. The shared
libraries it builds on are implemented (see `../../specs/libraries.md`).

## Implementation status

- **Done:**
  - Window with menu bar (`File > Open…`, `Quit`); opening a `.bm1` by dialog, drag and drop, or as the first command-line argument; loading on a background thread; status bar; empty, loading and failed screens. A composition whose samples are missing still opens (with warnings and no audio).
  - **B** tabs *Metadata* (the default) / *Samples* / *Patterns*. The two lists show names with a color chip (the sample's color); clicking selects. Each sample has a **play button** that sounds it once at its original pitch, and each pattern one that plays it once from its start (with its sample, notes and the composition's tempo); both sound on top of the transport, which is not touched.
  - **C** properties (vertical scroll) of the selected sample or pattern. A pattern shows its facts, the **step grid** (piano-roll style: one row per distinct pitch, one column per step, beats shaded and numbered) and where it is used; a sample shows status, root note, length, sample rate, file path and the patterns that use it.
  - **D** arrangement: one row per track with a **mute icon button** (speaker / crossed-out speaker) and name, and a step grid where every pattern is a filled block colored by its sample with its notes drawn inside, loop repetitions dimmed, column shading, beat lines, a pinned ruler (column numbers and start times) and pinned track column, shared 2D scroll, zoom slider, and click-to-select a block (which selects the pattern in C). Selecting a pattern highlights its blocks.
  - **Transport ribbon**, directly under the arrangement: one **play/pause toggle** (it shows what a click will do, and goes back to *play* by itself on stop or at the end), stop, loop (icons with tooltips), `elapsed / total` and a position slider (dragging seeks). Space toggles play/pause. It drives the `bm-playback` engine; the mute buttons and Loop are pushed to the engine every frame. A red **playback cursor** (line plus a marker on the ruler) crosses the arrangement, and the view scrolls to keep it visible while playing. Without an audio device (or without audio because samples are missing) the controls are disabled and the reason is shown; the app still works as a viewer.
  - **Status bar**: file, integrity, sample count on the left; on the right the **master volume** slider (transport and sample previews) and, to its right, the arrangement **zoom** slider.
- **Menus**: `File` (Open…, Quit), `Tools` (*Libraries…*, *Settings…*) and `Help` (*About…*). Each of the three opens a **modal** window closed with its Close button, Esc or a click outside. *Libraries* lists the internal `bm-*` libraries and the direct third-party dependencies with their versions, generated at build time from `Cargo.lock` (`gui-player/build.rs`), so it always matches what is linked. *Settings* is an empty placeholder. *About* shows the product ("bit-music gui-player"), its version, its license (MIT OR Apache-2.0) and a pointer to `THIRD_PARTY_LICENSES.md`.
- **Icon buttons** are square (1:1) with the icon centered (`widgets::IconButton`).
- **Icons** come from the Phosphor set (`egui-phosphor`, MIT), registered as a font at startup.
- **Startup errors are never silent.** The Windows release build has no console, so a failure to start (no usable graphics, window creation error) and any panic on the main thread are shown in a native error dialog (and printed to stderr). If the loading thread dies, the app shows a failure instead of "Loading…" forever.
- **Note on time:** the total shown by the transport is the length of the rendered audio, which can exceed the arrangement (the demo's arrangement is 2.5 s, but its piano note keeps ringing and the audio lasts about 4.2 s). The cursor is only drawn while it is inside the arrangement.
- **Next:** *File > Export WAV…* and the remaining status details; click-on-ruler seeking; a follow toggle.
- Tests: pure model tests (pattern grid), headless egui runs of every panel and of the arrangement, and load/state tests.

### Reviewing the UI without a screen

Two environment variables, meant for development only, let the UI be checked or documented without a person looking at the window:

- `BM_GUI_SCREENSHOT=<file.ppm>`: after a few frames, save a screenshot of the window and quit.
- `BM_GUI_SELECT=sample:<id>` or `pattern:<id>`: start with that element selected (so C has something to show).
- `BM_GUI_DIALOG=libraries|settings|about`: start with that modal open.
- `BM_GUI_PLAY_AT=<seconds>`: start playing from that position as soon as the composition is open (needs an audio device), to capture the transport and cursor in action.

To capture the Windows build from WSL, forward the variables with `WSLENV`, for example `WSLENV="BM_GUI_SCREENSHOT/p:BM_GUI_PLAY_AT:BM_GUI_SELECT"` (the `/p` converts the path to a Windows path).

Observation: under WSLg the Wayland connection failed about every second launch (`Io error: Broken pipe`); the same binary was reliable through X11 (`env -u WAYLAND_DISPLAY`).

## Layout

```
+----------------------------------------------------------------------+
| File:  Open...   Quit                                                |   menu bar
+---------------------------------------+------------------------------+
| B [Metadata][Samples][Patterns]       | C Properties                 |   top panel
|   metadata, or a list of names        |   detail of the item         |   (resizable)
|   ([>] plays each sample) (scroll V)  |   selected in B or D         |
|                                       |   (scroll V)                 |
+-----+----------------------------------------------------------------+
| Tracks  ruler:  1 00:00.0 | 2 00:01.5 | ...                          |
| [)] | kick   [kickA     ][kickA     ]                                |   D Arrangement
| [)] | snare  [snareA    ][snareA    ]                                |   (scroll V/H
| [/] | hihat  [hihatA    ][hihatA    ]                                |    shared)
| [)] | epiano [epianoA   ][epianoA (loop)]                            |
| [)] | sax    [saxA       ]                                           |
+----------------------------------------------------------------------+
| [>/||] [#] [@]   00:03.2 / 00:04.2   |=====o----------------------|   transport
+----------------------------------------------------------------------+
| song1.bm1 - integrity OK - 5/5 samples      Vol [--o--]  Zoom [--o--] |   status bar
+----------------------------------------------------------------------+
```

The arrangement has no title of its own (its corner just says "Tracks"). Legend: `[>/||]` play/pause toggle, `[#]` stop, `[@]` loop, `[)]` mute (speaker icon; `[/]` = crossed out, muted). The zoom slider lives at the right of the status bar, with the master volume to its left.

## Areas

- **Menu bar**: `File` with *Open…* and *Quit* (*Export WAV…* is planned).
- **B, tabs**: *Metadata* (default): title, format version, bpm, steps per beat, seconds per step, length, and the `others` key/value pairs. *Samples* and *Patterns*: lists of names with a chip in the sample's color (a sample whose file is missing is shown in red); clicking an element selects it and shows its detail in C. Every sample and every pattern also has a play button (disabled without audio). Vertical scroll.
- **C, properties**: the detail of what is selected in B, or of the pattern whose block was clicked in D. Vertical scroll.
  - A **sample** shows its status, root note, length, frames, sample rate, file path and the patterns that use it.
  - A **pattern** shows its facts (steps, beats, sounding steps, distinct pitches), the **step grid**, and the tracks that use it (noting repetitions that come from looping).
- **D, arrangement**: one row per track inside a single shared 2D scroll area (vertical and horizontal), so all tracks scroll together, with no title of its own.
  - **Track row** = mute icon + render. The icon is the mute toggle and stays pinned while the render scrolls horizontally. The render shows the track's patterns as blocks.
  - The column **ruler** stays pinned at the top.
- **Transport**, directly below the arrangement: play/pause toggle, stop, loop, elapsed/total time and a position (seek) bar over the whole song.
- **Status bar**: file name, integrity result, sample availability summary, master volume and the arrangement zoom sliders on the right.

## MVP scope

- [x] Open a `.bm1` and show metadata, samples, patterns and the arrangement's tracks.
- [x] Play, pause, stop, loop and seek, with a playback cursor moving across the arrangement.
- [x] Mute per track.
- [ ] Export to WAV (the same result as `bm export --wav`).
- [x] Report a composition that fails validation with its error message, and flag missing or invalid samples in the lists, the properties and the status bar (the same checks as `bm check`).

Out of scope: editing compositions (that is `gui-editor`).

## Refinements beyond the MVP

Implemented:

- Pattern blocks with width proportional to their steps, colored by sample (the color chips in B act as the legend), notes drawn inside, loop repetitions dimmed, and `null` gaps left empty.
- Horizontal zoom.
- Selection links: clicking a block in D selects its pattern in B and C, and the selected pattern's blocks are outlined.
- A "missing" marker for samples whose file is not usable.
- An empty state when no file is open ("open or drop a .bm1").

Not implemented yet:

- Solo per track (cheap now that each track has its own buffer).
- Highlighting in D the patterns of a sample selected in B.
- A collapsible top panel to give the arrangement the full height (it is resizable today).

## After the MVP

- Waveform of the mix.
- Automatic reload when the `.bm1` changes on disk (fits the future editor workflow).

## Framework and platform notes

- GUI framework: **egui/eframe**. A spike (a minimal eframe window with `cpal` linked) built and ran on Windows (mingw cross-compilation from WSL) and on Linux (Wayland and X11), with about 0.6-1.4 ms of UI CPU per frame for 16,000 unculled rectangles. Electron was analyzed and not chosen for the player; the library split keeps the door open to a different front end later.
- The Windows release build uses the `windows` subsystem, so no console window appears behind the GUI (which is why startup errors go to a dialog).
- **Running the unsigned Windows build:** Windows 11 Smart App Control can block it; Windows Developer Mode lets it run. See `../../specs/ci-and-signing.md`.
- `File > Open…` uses the native dialog through `rfd` (Win32 dialog on Windows; on Linux it goes through the desktop portal, so an `xdg-desktop-portal` service must be running).
- **Build gotcha (Windows target):** `wgpu-hal` and `gpu-allocator` must use the same `windows` crate version. `cpal 0.15` pins `windows 0.54`, and cargo may reuse it for `gpu-allocator` (which accepts `>=0.53, <=0.62`), breaking `wgpu-hal`'s DX12 code with "multiple versions of crate `windows`". `Cargo.lock` therefore points `gpu-allocator` to `windows 0.62.2`; if a `cargo update` reverts it, re-point that one entry (or upgrade `cpal`).
- Linux runtime needs the usual desktop libraries (Wayland/X11, Vulkan or GL); no development packages are needed to build.
