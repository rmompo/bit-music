# gui-player — layout and MVP scope

Desktop application (Windows, Ubuntu and similar) that opens a `.bm1`
composition, shows its contents and plays it. It is built on the same
shared libraries as the `bm` command-line player, and it is meant to evolve
into `gui-editor` (the same application with editing on top).

The executable is **`bm-gui`** (`bm-gui.exe` on Windows), built by the `gui-player` package: `cargo build -p gui-player`, `scripts/build-windows.sh gui-player`. The `bm` command-line player is `bm`. The configuration file keeps the name `gui-player.json`.

Status: **layout agreed; framework agreed (egui/eframe)**. The shared
libraries it builds on are implemented (see `../../specs/libraries.md`).

## Implementation status

- **Done:**
  - Window with menu bar (`File > Open`, `Quit`); opening a `.bm1` by dialog, drag and drop, or as the first command-line argument; loading on a background thread; status bar; empty, loading and failed screens. A composition whose samples are missing still opens (with warnings and no audio).
  - **A** tabs *Metadata* (the default) / *Samples* / *Patterns*. The two lists have one control per row (`[play] [color] name`): clicking anywhere on the row selects it, the play button only plays, and each tab keeps its own selection. Each sample has a **play button** that sounds it once at its original pitch, and each pattern one that plays it once from its start (with its sample, notes and the composition's tempo); both sound on top of the transport, which is not touched. An element's play button stays disabled while its own preview is sounding, so plays cannot pile up.
  - **B** properties of the selected sample or pattern, in **two equal columns** (a fixed 50% / 50%), each with its own vertical scroll. The left one has all the properties: first what is stored in the `.bm1` (a sample's id, root note and file exactly as written; a pattern's id and sample, without its steps), then what is calculated (a sample's status, length, frames and sample rate; a pattern's step count, beats, sounding steps and distinct pitches). The right one only says where it is used (*Used by patterns* / *Used in tracks*) and, for a pattern, shows the **step grid** (piano-roll style like a DAW's: every semitone from the start of the lowest octave the pattern uses to the end of the highest one, highest on top, with a keyboard on the left (white keys light, black keys dark and shorter, each with its name such as `C#4`), the rows of the white keys a little lighter than those of the black ones, one column per step, beats shaded and numbered. The small preview inside an arrangement block uses the same full scale (the whole height of the block is the whole range of octaves), so a note's height means the same there and in the grid; it scrolls horizontally on its own). With the *Metadata* tab open, B shows the metadata's `others` instead.
  - **C** arrangement: one row per track with a **mute icon button** (speaker / crossed-out speaker) and name, and a step grid where every pattern is a filled block colored by its sample with its notes drawn inside, loop repetitions dimmed, column shading, beat lines, a pinned ruler (column numbers and start times) and pinned track column, shared 2D scroll, zoom slider, and click-to-select a block (which selects the pattern in A and B). Selecting a pattern highlights its blocks.
  - **Transport ribbon**, directly under the arrangement: one **play/pause toggle** (it shows what a click will do, and goes back to *play* by itself on stop or at the end), stop, loop (icons with tooltips), `elapsed / total` and a position slider (dragging seeks). Space toggles play/pause. It drives the `bm-playback` engine; the mute buttons and Loop are pushed to the engine every frame. A red **playback cursor** (line plus a marker on the ruler) crosses the arrangement, and the view scrolls to keep it visible while playing. Without an audio device (or without audio because samples are missing) the controls are disabled and the reason is shown; the app still works as a viewer.
  - **Status bar** (the footer), left to right: file, integrity and sample count; then, after a separator that is always there and in all the space left, a red `bug` icon that opens the errors window and, at its right, the **latest error** (red, cut with "…" if it does not fit, full text on hover) — both only while there are errors; then, on the right, the **master volume** (a mute button, then the slider; it affects the transport and the sample previews) and the arrangement **zoom** slider. The mute button shows a speaker, or a crossed-out one while muted; muting sets the volume to 0 and remembers the previous volume, clicking again goes back to it, and dragging the slider above 0 un-mutes too (muted simply means a volume of 0).
- **Menus**: `File` (*Open*, *Open recent* with the history, *Quit*, which asks for confirmation in a modal, as does the window's close button), `Tools` (*Export > WAV*, *Settings*) and `Help` (*Libraries*, *About*). *Export > WAV* (enabled when the composition has audio) asks where to save, starting in the `path` setting's folder and proposing `<composition>.wav`, writes the full mix (the same audio as `bm export --wav`) and reports the result in a modal. Each menu entry that opens a **modal** window has its action buttons always at the bottom right, under a horizontal line that separates them from the content, and is closed with its Close button, Esc or a click outside. *Libraries* lists the internal `bm-*` libraries and the direct third-party dependencies with their versions, generated at build time from `Cargo.lock` (`gui-player/build.rs`), so it always matches what is linked. *About* shows the product ("bit-music gui-player"), its version, its license (MIT OR Apache-2.0) and a pointer to `THIRD_PARTY_LICENSES.md`.
- **Configuration**: `gui-player.json`, next to the executable, created with the defaults when missing. It holds only *values*: `settings` (a list of `{key, value}`, typed: numbers and booleans as such) and `lastOpened`, the history of successfully opened compositions (most recent first, absolute paths as `{key, value}`; reopening a file moves it to the top and entries beyond `maxLastOpened` are dropped). Everything else about a setting is defined in `gui-player/gui-player.schema.json`, embedded in the executable:
  - `settingType`: `USER` (editable in Tools > Settings) or `SYSTEM` (state the app keeps for itself: window, dividers).
  - `settingTitle`, `settingDescription`, `dataType` (`integer`, `boolean`, `string`) and `controlType` (`input`, `spinner`, `slider`, `checkbox`, `combo`; a `USER` setting needs one, and it must fit the data type: integers take a spinner, slider or combo; text an input or combo; booleans a checkbox; a combo picks one of `enumValue`, whether the values are numbers or text).
  - `values`: `minValue`, `maxValue`, `enumValue` (the choices of a combo) and `defaultValue` (mandatory for `USER`; absent for `windowX`/`windowY`, which have no meaningful default).
  A test checks the schema's coherence. Missing or invalid values (out of limits, wrong type) are reset to their default, or dropped when there is none; text such as `"10"` from older files is accepted; unknown settings are kept. A corrupt file is reported on stderr and left untouched (defaults are used in memory).
- **Tools > Settings** is generated from the schema (one row per `USER` setting: title, description, the control, and a *Restore default* button). It edits a copy: **OK** applies and saves it, **Cancel**, Esc or a click outside discards it, and **Reset all to defaults** puts every `USER` setting back to its default (in the copy, until OK). A *Clear history* button empties `lastOpened`. The `USER` settings are: `lang` (combo: `ENGLISH` by default, or `SPANISH`; applied as soon as you press OK), `maxLastOpened` (combo: 5, 10, 15 or 20; default 10) and `path` (text: the folder file dialogs, *Open* and *Export*, start in; default `C:\LocalFiles\proyectos\personal\bit-music\demos\songs\`, ignored if it is not an existing folder).
- **Dividers**: the vertical one between A and B and the horizontal one between A + B and C can be dragged; they are drawn by the app itself and sized from the stored percentage, not from egui's remembered panel sizes. Each side keeps at least 120 points. Positions are the `SYSTEM` settings `tabsWidthPercent` (A's share of the width of A + B; 30% by default, limited to 30%–50%) and `arrangementHeightPercent` (C's share of the height; 50% by default, limited to 50%–75%). The limits are the `minValue`/`maxValue` of those schema entries.
- **Window state**: the settings `windowMaximized` (default `true`), `windowX`/`windowY` (optional), `windowWidth` and `windowHeight` record how the window was left, and it opens that way. While maximized only the flag changes, so restoring returns to the last restored geometry. Changes are written about 0.6 s after the last one. Where the system does not report a window position (Wayland), no position is stored and the system places the window.
- **Live oscilloscope**: while a sample or pattern preview sounds, its list row shows the audio being played as a faint trace across the whole row, behind the contents; while the transport plays, each track's cell in the pinned TRACK column shows that track's audio across the whole column, behind the mute button and the name (nothing for a muted track, or when stopped or paused). Both are the window of audio (about 120 ms) around the current position, read from the audio already in memory (`bm-playback`'s `track_scope` and `preview_scope`); they follow the master volume, and the audio thread is not involved.
- **Languages (i18n)**: every text of the interface is looked up by key in `gui-player/i18n/en.json` or `es.json` (flat JSON, embedded in the executable; `{name}` placeholders; a missing key falls back to English, then to the key). The language is the `lang` setting and changes live. Texts of the `USER` settings are `setting.<key>.title` / `.description`, and the words of a combo are `value.<key>.<choice>` (each language name is written in its own language). Tests check that both files have the same keys and placeholders, that every key used in the sources exists, and that every user setting is translated. Not translated: the product name, messages that come from libraries (audio device errors, format validation), and startup errors (shown before the configuration is read).
- **Errors**: everything that goes wrong while the app runs is kept in a log, a **stack with the newest on top**: no audio device or an audio stream that fails, a composition that cannot be opened (its central failure screen stays, and it is also logged), a failed export, and configuration that cannot be read or saved. An error identical to the one on top is not added again. The footer shows the latest; the `bug` icon opens the **Errors** window, of a **fixed size** (it does not change when errors are removed; the list scrolls vertically): each error is one **row**, a soft card without lines, with its date and time (`DD/MM/YYYY HH:MM:SS`, the system's local time) over its text (as many lines as it needs) and, at the right, an `x-circle` button that removes it, centered vertically on the time and text together; *Clear all* and *Close* are at the bottom right. The log lives while the app is open, is not emptied by opening another composition, and only the user clears it. Without audio, the transport's position slider is shown disabled and no message; a successful export is still reported in a dialog.
- **Application icon**: Phosphor's `file-audio` in white on a rounded blue tile. It is the window and taskbar icon (`assets/icon-128.rgba` at the repository root, set with `with_icon`) and is embedded in the Windows executables, both `bm-gui.exe` and `bm.exe` (`assets/icon.ico`, several sizes, added by each crate's `build.rs` through `assets/windows_icon.rs` with the mingw `windres`; if that is missing the build only warns and the executable has no icon). `scripts/gen-app-icon.py` makes both from a capture of the glyph, which the `BM_GUI_ICON=1` dev aid draws (steps in the script's header); there is no image library involved.
- **Icon buttons** are square (1:1) with the icon centered (`widgets::IconButton`).
- **Icons** come from the Phosphor set (`egui-phosphor`, MIT), registered as a font at startup.
- **Startup errors are never silent.** The Windows release build has no console, so a failure to start (no usable graphics, window creation error) and any panic on the main thread are shown in a native error dialog (and printed to stderr). If the loading thread dies, the app shows a failure instead of "Loading…" forever.
- **Note on time:** the total shown by the transport is the length of the rendered audio, which can exceed the arrangement (the demo's arrangement is 2.5 s, but its piano note keeps ringing and the audio lasts about 4.2 s). The cursor is only drawn while it is inside the arrangement.
- **Next:** *File > Export WAV…* and the remaining status details; click-on-ruler seeking; a follow toggle.
- Tests: pure model tests (pattern grid), headless egui runs of every panel and of the arrangement, and load/state tests.

### Reviewing the UI without a screen

Two environment variables, meant for development only, let the UI be checked or documented without a person looking at the window:

- `BM_GUI_SCREENSHOT=<file.ppm>`: after a few frames, save a screenshot of the window and quit.
- `BM_GUI_SELECT=sample:<id>`, `pattern:<id>` or `tab:metadata|samples|patterns`: start with that element selected, or that tab open with nothing selected.
- `BM_GUI_DIALOG=libraries|settings|about|quit|errors`: start with that modal open.
- `BM_GUI_ICON=1`: draw only the icon's glyph, large (see the application icon below).
- `BM_GUI_NO_ERRORS=1`: keep the error log empty (with `BM_GUI_SCREENSHOT`), to see the footer without errors.
- `BM_GUI_VOLUME=<0..1>`: start with that master volume (0 is muted).
- `BM_GUI_LANG=ENGLISH|SPANISH`: force the interface language without touching the configuration.
- `BM_GUI_PREVIEW=sample:<id>` or `pattern:<id>`: start that preview as soon as the composition is open (needs an audio device), to capture the live oscilloscope of a list row.
- `BM_GUI_PLAY_AT=<seconds>`: start playing from that position as soon as the composition is open (needs an audio device), to capture the transport and cursor in action.

To capture the Windows build from WSL, forward the variables with `WSLENV`, for example `WSLENV="BM_GUI_SCREENSHOT/p:BM_GUI_PLAY_AT:BM_GUI_SELECT"` (the `/p` converts the path to a Windows path).

Observation: under WSLg the Wayland connection failed about every second launch (`Io error: Broken pipe`); the same binary was reliable through X11 (`env -u WAYLAND_DISPLAY`).

## Layout

```
+----------------------------------------------------------------------+
| File:  Open...   Quit                                                |   menu bar
+---------------------------------------+------------------------------+
| A [Metadata][Samples][Patterns]       | B Properties                 |   top panel
|   metadata, or a list of names        |   detail of the item         |   (resizable)
|   ([>] plays each sample) (scroll V)  |   selected in A or C         |
|                                       |   (scroll V)                 |
+-----+----------------------------------------------------------------+
| Tracks  ruler:  1 00:00.0 | 2 00:01.5 | ...                          |
| [)] | kick   [kickA     ][kickA     ]                                |   C Arrangement
| [)] | snare  [snareA    ][snareA    ]                                |   (scroll V/H
| [/] | hihat  [hihatA    ][hihatA    ]                                |    shared)
| [)] | epiano [epianoA   ][epianoA (loop)]                            |
| [)] | sax    [saxA       ]                                           |
+----------------------------------------------------------------------+
| [>/||] [#] [@]   00:03.2 / 00:04.2   |=====o----------------------|   transport
+----------------------------------------------------------------------+
| song1.bm1 | integrity OK | 5/5 samples | [bug] no audio: could not get the def… | Vol [--o--] | Zoom [--o--] |   status bar
+----------------------------------------------------------------------+
```

The arrangement has no title of its own (its corner just says "Tracks"). Legend: `[>/||]` play/pause toggle, `[#]` stop, `[@]` loop, `[)]` mute (speaker icon; `[/]` = crossed out, muted). The zoom slider lives at the right of the status bar, with the master volume to its left.

## Areas

- **Menu bar**: `File` with *Open*, *Open recent* and *Quit* (*Export WAV…* is planned).
- **A, tabs**: *Metadata* (default): title, format version, bpm, steps per beat, seconds per step and length (the `others` key/value pairs are shown in B while this tab is open). *Samples* and *Patterns*: lists whose rows are single controls, `[play] [color] name`: a click anywhere on the row selects it (and highlights all of it), except on the play button, which only plays. A sample whose file is missing is shown in red. **Each tab has its own selection**, kept when switching tabs; opening a tab loads Properties for that tab's selection, or *Select an element to see its properties.* when there is none. Vertical scroll.
- **B, properties**: with the *Metadata* tab open, the metadata's `others`; otherwise the detail of what is selected in the open tab (a click on a block in C selects that pattern in the Patterns tab and opens it), in two equal columns (a fixed 50% / 50%), each with its own vertical scroll. The left one has all the properties (first what is stored in the `.bm1`, then what is calculated); the right one only where it is used and, for a pattern, its steps.
  - A **sample**: left, its id, root note and file as stored, then its status, length, frames and sample rate; right, *Used by patterns*.
  - A **pattern**: left, its id and sample, then its steps, beats, sounding steps and distinct pitches; right, *Used in tracks* (noting repetitions that come from looping) and below it the **step grid** with its own scroll.
- **C, arrangement**: one row per track inside a single shared 2D scroll area (vertical and horizontal), so all tracks scroll together, with no title of its own.
  - **Track row** = mute icon + render. The icon is the mute toggle and stays pinned while the render scrolls horizontally. The render shows the track's patterns as blocks.
  - The column **ruler** stays pinned at the top.
- **Transport**, directly below the arrangement: play/pause toggle, stop, loop, elapsed/total time and a position (seek) bar over the whole song.
- **Status bar**: file name, integrity result and sample availability summary on the left; the latest error and the errors icon in the middle; master volume and the arrangement zoom sliders on the right.

## MVP scope

- [x] Open a `.bm1` and show metadata, samples, patterns and the arrangement's tracks.
- [x] Play, pause, stop, loop and seek, with a playback cursor moving across the arrangement.
- [x] Mute per track.
- [ ] Export to WAV (the same result as `bm export --wav`).
- [x] Report a composition that fails validation with its error message, and flag missing or invalid samples in the lists, the properties and the status bar (the same checks as `bm check`).

Out of scope: editing compositions (that is `gui-editor`).

## Refinements beyond the MVP

Implemented:

- Pattern blocks with width proportional to their steps, colored by sample (the color chips in A act as the legend), notes drawn inside, loop repetitions dimmed, and `null` gaps left empty.
- Horizontal zoom.
- Selection links: clicking a block in C selects its pattern in A and B, and the selected pattern's blocks are outlined.
- A "missing" marker for samples whose file is not usable.
- An empty state when no file is open ("open or drop a .bm1").

Not implemented yet:

- Solo per track (cheap now that each track has its own buffer).
- Highlighting in C the patterns of a sample selected in A.
- A collapsible top panel to give the arrangement the full height (it is resizable today).

## After the MVP

- Waveform of the mix.
- Automatic reload when the `.bm1` changes on disk (fits the future editor workflow).

## Framework and platform notes

- GUI framework: **egui/eframe**. A spike (a minimal eframe window with `cpal` linked) built and ran on Windows (mingw cross-compilation from WSL) and on Linux (Wayland and X11), with about 0.6-1.4 ms of UI CPU per frame for 16,000 unculled rectangles. Electron was analyzed and not chosen for the player; the library split keeps the door open to a different front end later.
- The Windows release build uses the `windows` subsystem, so no console window appears behind the GUI (which is why startup errors go to a dialog).
- **Running the unsigned Windows build:** Windows 11 Smart App Control can block it; Windows Developer Mode lets it run. See `../../specs/ci-and-signing.md`.
- `File > Open` uses the native dialog through `rfd` (Win32 dialog on Windows; on Linux it goes through the desktop portal, so an `xdg-desktop-portal` service must be running).
- **Build gotcha (Windows target):** `wgpu-hal` and `gpu-allocator` must use the same `windows` crate version. `cpal 0.15` pins `windows 0.54`, and cargo may reuse it for `gpu-allocator` (which accepts `>=0.53, <=0.62`), breaking `wgpu-hal`'s DX12 code with "multiple versions of crate `windows`". `Cargo.lock` therefore points `gpu-allocator` to `windows 0.62.2`; if a `cargo update` reverts it, re-point that one entry (or upgrade `cpal`).
- Linux runtime needs the usual desktop libraries (Wayland/X11, Vulkan or GL); no development packages are needed to build.
