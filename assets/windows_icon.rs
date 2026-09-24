// Shared by the build scripts of `bm` and `bm-gui` (`include!`d into each).
//
// Puts `assets/icon.ico` into the Windows executable (the icon Explorer and
// the taskbar show for the file), using the mingw `windres` that cross
// compiling already needs. Without it the executable is built without the
// icon and a warning says so. `manifest` is the directory of the crate being
// built, one level below `assets/`.

fn embed_windows_icon(manifest: &std::path::Path) {
    let icon = manifest.join("../assets/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::env::var("OUT_DIR").unwrap();
    let rc = std::path::Path::new(&out).join("icon.rc");
    let object = std::path::Path::new(&out).join("icon.res.o");
    let icon_path = icon.canonicalize().unwrap_or(icon);
    std::fs::write(
        &rc,
        format!("1 ICON \"{}\"\n", icon_path.display().to_string().replace('\\', "/")),
    )
    .unwrap();

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let windres = format!("{}-w64-mingw32-windres", if arch == "x86" { "i686" } else { "x86_64" });
    match std::process::Command::new(&windres)
        .args(["-O", "coff", "-i"])
        .arg(&rc)
        .arg("-o")
        .arg(&object)
        .status()
    {
        Ok(status) if status.success() => {
            println!("cargo:rustc-link-arg-bins={}", object.display());
        }
        other => println!("cargo:warning=could not embed the icon with {windres}: {other:?}"),
    }
}
