# bit-music

A JSON-based music composition format (reusable sound "chunks" chained
across parallel tracks) plus the tools to edit and play it.

The shared contract between all components — the shape of the JSON file —
is documented in [`specs/format.md`](specs/format.md). Logic common to more
than one component lives in reusable libraries under [`libs/`](libs/),
described in [`specs/libraries.md`](specs/libraries.md). Each component
documents its own implementation decisions in its own `specs/` folder.

The demo audio is synthesized from code, not recorded or downloaded: see
[`tools/gen-demo-samples`](tools/gen-demo-samples/) and
[`player/demos/samples/`](player/demos/samples/).

Build with the scripts in [`scripts/`](scripts/) (for example
`scripts/build-windows.sh`), or `cargo test --workspace` to run every test.

## Components

### [`player/`](player/)
Command-line player, built as the `bm` executable. `bm play song.bm1` plays a
composition (`--non-stop` loops it), `bm export song.bm1 --wav` renders it to a
`.wav`, and `bm check-integrity`, `check-samples` and `check` validate it.
`bm version` and `bm help` describe the tool. Written in Rust, compiles to a
native executable (Windows and Linux). See [`player/specs/`](player/specs/) for
implementation details.

### [`gui-player/`](gui-player/)
Player with a graphical interface (egui/eframe, Windows and Linux). It opens a
composition and shows its metadata, its samples and patterns (with a step grid
for each pattern) and the arrangement with per-track mute, and plays it with a
transport ribbon and a cursor. Exporting to WAV from the interface is still to
come. See [`gui-player/specs/`](gui-player/specs/).

### [`gui-editor/`](gui-editor/)
Editor for creating and modifying compositions without hand-writing the
JSON: the gui-player with editing on top. *(Not implemented yet.)*

### [`tools/`](tools/)
Development tools. `gen-demo-samples` generates the demo audio from code.

## Windows builds

There is no CI workflow at the moment. To build the Windows executables, use
`scripts/build-windows.sh` (and `scripts/package-windows.sh` for a zip).

**The Windows binaries are not code-signed.** Windows 11 Smart App Control and
SmartScreen can block unsigned programs; on a development machine, turning on
Windows Developer Mode is enough to run them. How to bring back the CI and how
to get signed binaries (SignPath Foundation) is documented in
[`specs/ci-and-signing.md`](specs/ci-and-signing.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

The licenses of the third-party libraries this project builds on are listed
in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
