//! Puts the application icon into the Windows executable (`bm.exe`), the same
//! one `bm-gui.exe` has.

fn main() {
    embed_windows_icon(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
}

include!("../assets/windows_icon.rs");
