//! Generates the list of libraries shown in *Tools > Libraries*, with the
//! exact versions the binary is built with, read from the workspace's
//! `Cargo.lock` (so the list can never drift from what is linked).
//!
//! Internal libraries (`bm-*`) are the ones reachable from this crate;
//! third-party ones are the direct dependencies of this crate and of those
//! internal libraries.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

struct Package {
    version: String,
    dependencies: Vec<String>,
}

fn main() {
    let lock_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    println!("cargo:rerun-if-changed=build.rs");
    let lock = std::fs::read_to_string(&lock_path).expect("could not read Cargo.lock");

    let packages = parse(&lock);
    let root = "gui-player";

    // Internal libraries reachable from this crate.
    let mut internal = BTreeSet::new();
    let mut pending = vec![root.to_string()];
    while let Some(name) = pending.pop() {
        let Some(pkg) = packages.get(&name) else { continue };
        for dep in &pkg.dependencies {
            if dep.starts_with("bm-") && internal.insert(dep.clone()) {
                pending.push(dep.clone());
            }
        }
    }

    // Third-party: direct dependencies of this crate and of the internal libs.
    let mut third_party = BTreeSet::new();
    for owner in std::iter::once(root.to_string()).chain(internal.iter().cloned()) {
        if let Some(pkg) = packages.get(&owner) {
            for dep in &pkg.dependencies {
                if !dep.starts_with("bm-") {
                    third_party.insert(dep.clone());
                }
            }
        }
    }

    let render = |names: &BTreeSet<String>| {
        names
            .iter()
            .filter_map(|n| packages.get(n).map(|p| format!("    ({n:?}, {:?}),\n", p.version)))
            .collect::<String>()
    };
    let code = format!(
        "/// (name, version) of the internal libraries linked into this binary.\n\
         pub const INTERNAL: &[(&str, &str)] = &[\n{}];\n\
         /// (name, version) of the direct third-party dependencies.\n\
         pub const THIRD_PARTY: &[(&str, &str)] = &[\n{}];\n",
        render(&internal),
        render(&third_party)
    );
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("libraries.rs");
    std::fs::write(out, code).unwrap();
}

/// Minimal `Cargo.lock` reader: `name`, `version` and the dependency names
/// of each `[[package]]`. When a crate appears in several versions the
/// first one wins, which is enough for display.
fn parse(lock: &str) -> BTreeMap<String, Package> {
    let mut packages: BTreeMap<String, Package> = BTreeMap::new();
    for block in lock.split("[[package]]").skip(1) {
        let mut name = None;
        let mut version = None;
        let mut dependencies = Vec::new();
        let mut in_deps = false;
        for line in block.lines() {
            let line = line.trim();
            if in_deps {
                if line == "]" {
                    in_deps = false;
                } else if let Some(entry) = line.trim_end_matches(',').strip_prefix('"') {
                    let entry = entry.trim_end_matches('"');
                    dependencies.push(entry.split(' ').next().unwrap_or(entry).to_string());
                }
            } else if let Some(v) = line.strip_prefix("name = ") {
                name = Some(v.trim_matches('"').to_string());
            } else if let Some(v) = line.strip_prefix("version = ") {
                version = Some(v.trim_matches('"').to_string());
            } else if line == "dependencies = [" {
                in_deps = true;
            }
        }
        if let (Some(name), Some(version)) = (name, version) {
            packages.entry(name).or_insert(Package { version, dependencies });
        }
    }
    packages
}
