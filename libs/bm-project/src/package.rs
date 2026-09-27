//! Reading and writing `.bmz` packages: a `.bm1` plus every sample it
//! references, bundled as a plain zip so a composition can travel without
//! depending on where its samples happen to live on the author's disk.
//!
//! Layout of a `.bmz`:
//! - `bm1/<name>.bm1`: the composition, with every `sample.file` rewritten
//!   to a path relative to it, always under `../samples/`.
//! - `samples/...`: every sample that could be read when the package was
//!   written, preserving the folder structure it was declared with when
//!   that stays under the composition's own directory; anything absolute,
//!   or that reaches outside that directory with `..`, is flattened to a
//!   short `<folder>/<file>` path instead (extended with more ancestors,
//!   or as a last resort suffixed, only if that still collides with
//!   another sample) — so nothing ever ends up outside `samples/`.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

use crate::{Project, ProjectError, SampleSource};

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("could not write '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write package '{path}': {source}")]
    Zip {
        path: String,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("could not serialize the composition: {0}")]
    Format(#[from] serde_json::Error),
}

/// What [`write_package`] did.
#[derive(Debug)]
pub struct PackageReport {
    pub path: PathBuf,
    /// Ids of samples the composition references but whose bytes could not
    /// be read (missing from disk, or from the archive `project` itself
    /// came from). They are still referenced from the packaged `.bm1`
    /// (exactly like a missing sample is still listed today), just not
    /// included in the zip.
    pub skipped: Vec<String>,
}

/// Bundles `project` — its `.bm1`, rewritten, and every sample it can
/// read — into a `.bmz` at `out_path`.
pub fn write_package(project: &Project, out_path: &Path) -> Result<PackageReport, PackageError> {
    let destinations = plan_sample_destinations(project);

    let mut composition = project.composition.clone();
    for sample in &mut composition.samples {
        if let Some(dest) = destinations.get(&sample.id) {
            sample.file = format!("../samples/{dest}");
        }
    }
    let bm1_json = bm_format::to_json(&composition)?;
    let bm1_name = project
        .path
        .file_stem()
        .map_or_else(|| "composition".to_string(), |s| s.to_string_lossy().into_owned());

    let path_str = out_path.display().to_string();
    let to_io_err = |source| PackageError::Io { path: path_str.clone(), source };
    let to_zip_err = |source| PackageError::Zip { path: path_str.clone(), source };

    let file = std::fs::File::create(out_path).map_err(to_io_err)?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file(format!("bm1/{bm1_name}.bm1"), options).map_err(to_zip_err)?;
    zip.write_all(bm1_json.as_bytes()).map_err(to_io_err)?;

    let mut skipped = Vec::new();
    for sample in &project.composition.samples {
        let Some(dest) = destinations.get(&sample.id) else { continue };
        match project.source.read(&sample.file) {
            Ok(bytes) => {
                zip.start_file(format!("samples/{dest}"), options).map_err(to_zip_err)?;
                zip.write_all(&bytes).map_err(to_io_err)?;
            }
            Err(_) => skipped.push(sample.id.clone()),
        }
    }

    zip.finish().map_err(to_zip_err)?;
    Ok(PackageReport { path: out_path.to_path_buf(), skipped })
}

/// Loads a project from a `.bmz` already read into `bytes`. `display_path`
/// is kept as `Project.path` (for titles and export file names) but is not
/// otherwise used: samples are resolved purely inside the archive.
pub fn load_bmz(bytes: &[u8], display_path: PathBuf) -> Result<Project, ProjectError> {
    let label = display_path.display().to_string();
    let to_zip_err = |source| ProjectError::Zip { path: label.clone(), source };

    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(to_zip_err)?;
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let bm1_name = names
        .iter()
        .find(|n| n.starts_with("bm1/") && n.to_ascii_lowercase().ends_with(".bm1"))
        .or_else(|| names.iter().find(|n| n.to_ascii_lowercase().ends_with(".bm1")))
        .cloned()
        .ok_or_else(|| ProjectError::NoComposition { path: label.clone() })?;

    let text = {
        use std::io::Read;
        let mut file = archive.by_name(&bm1_name).map_err(to_zip_err)?;
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|source| ProjectError::Zip { path: label.clone(), source: zip::result::ZipError::Io(source) })?;
        text
    };

    let mut composition = bm_format::parse_composition(&text)?;
    let declared_files: HashMap<String, String> = composition
        .samples
        .iter()
        .map(|s| (s.id.clone(), s.file.clone()))
        .collect();

    let base_dir = zip_parent(&bm1_name);
    for sample in &mut composition.samples {
        sample.file = resolve_zip_relative(base_dir, &sample.file);
    }

    // Decompressed eagerly: packages are small enough, and this keeps
    // `Project` self-contained instead of holding the archive open.
    let mut entries = HashMap::new();
    for name in &names {
        if *name == bm1_name || name.ends_with('/') {
            continue;
        }
        use std::io::Read;
        let mut file = archive.by_name(name).map_err(to_zip_err)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| ProjectError::Io { path: label.clone(), source })?;
        entries.insert(name.clone(), bytes);
    }

    Ok(Project {
        path: display_path,
        composition,
        declared_files,
        source: SampleSource::Archive(Arc::new(entries)),
    })
}

/// The directory part of a zip entry name (`"bm1/song.bm1"` -> `"bm1"`;
/// `""` for one with no directory).
fn zip_parent(entry: &str) -> &str {
    entry.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Resolves `file` (as declared in a packaged `.bm1`) against `base` (the
/// bm1 entry's own directory inside the zip), as pure path arithmetic on
/// `/`-separated zip entry names — no real file system is involved.
fn resolve_zip_relative(base: &str, file: &str) -> String {
    let normalized = file.replace('\\', "/");
    let mut segments: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            seg => segments.push(seg),
        }
    }
    segments.join("/")
}

/// For each sample id, the path (using `/`, never leading or containing
/// `..`) it gets under `samples/` in a `.bmz`.
fn plan_sample_destinations(project: &Project) -> HashMap<String, String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut dest = HashMap::new();
    for sample in &project.composition.samples {
        let (full, min_tail) = match shape_for(project, sample) {
            Shape::Contained(components) => {
                let len = components.len();
                (components, len)
            }
            // Start short (just the immediate folder and the file name);
            // `unique_tail` grows it only if that collides.
            Shape::Flattened(components) => (components, 2),
        };
        let chosen = unique_tail(&full, min_tail, &mut used);
        dest.insert(sample.id.clone(), chosen.join("/"));
    }
    dest
}

enum Shape {
    /// Stays under the composition's own directory (or, for an archive
    /// already loaded from a `.bmz`, is already a clean `samples/...`
    /// entry): keep the structure as-is.
    Contained(Vec<String>),
    /// Absolute, or escapes the composition's directory with `..`: only
    /// the file name is guaranteed meaningful, so ancestors are added one
    /// at a time (by [`unique_tail`]) until the destination is unique.
    Flattened(Vec<String>),
}

fn shape_for(project: &Project, sample: &bm_format::Sample) -> Shape {
    match &project.source {
        SampleSource::Archive(_) => {
            let tail = sample.file.strip_prefix("samples/").unwrap_or(&sample.file);
            Shape::Contained(tail.split('/').map(str::to_string).collect())
        }
        SampleSource::Disk => {
            let declared = project
                .declared_files
                .get(&sample.id)
                .map(String::as_str)
                .unwrap_or(&sample.file);
            let p = Path::new(declared);
            let escapes = p.is_absolute()
                || p.components()
                    .any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)));
            if escapes {
                // The declared path carries no usable structure (or leaves
                // the composition's directory); flatten from where the
                // file actually is.
                Shape::Flattened(normal_components(&sample.file))
            } else {
                Shape::Contained(normal_components(declared))
            }
        }
    }
}

fn normal_components(path: &str) -> Vec<String> {
    Path::new(path)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Picks a unique destination for `full` (path components, root to leaf):
/// starts with its last `min_tail` components (clamped to what is
/// available), growing by one more ancestor at a time while that collides
/// with an already-used destination, and finally falling back to a
/// numbered suffix on the file name.
fn unique_tail(full: &[String], min_tail: usize, used: &mut HashSet<String>) -> Vec<String> {
    let mut tail = min_tail.max(1).min(full.len().max(1));
    loop {
        let candidate = full[full.len().saturating_sub(tail)..].to_vec();
        if used.insert(candidate.join("/")) {
            return candidate;
        }
        if tail >= full.len() {
            break;
        }
        tail += 1;
    }
    let mut n: u32 = 2;
    loop {
        let mut candidate = full.to_vec();
        if let Some(last) = candidate.last_mut() {
            *last = suffixed(last, n);
        }
        if used.insert(candidate.join("/")) {
            return candidate;
        }
        n += 1;
    }
}

fn suffixed(file_name: &str, n: u32) -> String {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}-{n}.{ext}"),
        None => format!("{file_name}-{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SampleSource;
    use bm_format::Composition;
    use std::collections::HashMap as StdHashMap;

    fn demo_project() -> Project {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/songs/song1.bm1");
        crate::load(&path).unwrap()
    }

    #[test]
    fn resolve_zip_relative_undoes_the_bm1_samples_layout() {
        assert_eq!(resolve_zip_relative("bm1", "../samples/drums/kick.wav"), "samples/drums/kick.wav");
        assert_eq!(resolve_zip_relative("bm1", "../samples/kick.wav"), "samples/kick.wav");
        assert_eq!(resolve_zip_relative("", "kick.wav"), "kick.wav");
    }

    #[test]
    fn write_then_load_round_trips_every_sample() {
        let project = demo_project();
        let out = std::env::temp_dir().join(format!("bm-project-pkg-{}.bmz", std::process::id()));

        let report = write_package(&project, &out).unwrap();
        assert!(report.skipped.is_empty());

        let bytes = std::fs::read(&out).unwrap();
        std::fs::remove_file(&out).ok();
        let repackaged = load_bmz(&bytes, out).unwrap();

        assert_eq!(repackaged.composition.metadata.title, project.composition.metadata.title);
        assert_eq!(repackaged.composition.samples.len(), project.composition.samples.len());
        assert!(matches!(repackaged.source, SampleSource::Archive(_)));

        // Every sample resolves to real bytes inside the package, matching
        // what is on disk in the original project.
        for sample in &repackaged.composition.samples {
            let packaged_bytes = repackaged.source.read(&sample.file).unwrap();
            let original = project.composition.samples.iter().find(|s| s.id == sample.id).unwrap();
            let disk_bytes = std::fs::read(&original.file).unwrap();
            assert_eq!(packaged_bytes, disk_bytes);
        }
    }

    #[test]
    fn repackaging_an_already_packaged_project_keeps_the_same_structure() {
        let project = demo_project();
        let out1 = std::env::temp_dir().join(format!("bm-project-pkg1-{}.bmz", std::process::id()));
        write_package(&project, &out1).unwrap();
        let bytes1 = std::fs::read(&out1).unwrap();
        std::fs::remove_file(&out1).ok();
        let repackaged_project = load_bmz(&bytes1, out1).unwrap();

        let out2 = std::env::temp_dir().join(format!("bm-project-pkg2-{}.bmz", std::process::id()));
        write_package(&repackaged_project, &out2).unwrap();
        let bytes2 = std::fs::read(&out2).unwrap();
        std::fs::remove_file(&out2).ok();
        let twice_repackaged = load_bmz(&bytes2, out2).unwrap();

        let mut a: Vec<&String> = repackaged_project.composition.samples.iter().map(|s| &s.file).collect();
        let mut b: Vec<&String> = twice_repackaged.composition.samples.iter().map(|s| &s.file).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    fn sample(id: &str, file: &str) -> bm_format::Sample {
        bm_format::Sample { id: id.to_string(), file: file.to_string(), root_note: None, root_octave: None }
    }

    fn minimal_composition(samples: Vec<bm_format::Sample>) -> Composition {
        Composition {
            metadata: bm_format::Metadata {
                version: "1.0".to_string(),
                title: "t".to_string(),
                bpm: 120,
                steps_per_beat: 4,
                others: Vec::new(),
            },
            samples,
            patterns: Vec::new(),
            arrangement: bm_format::Arrangement { tracks: Vec::new() },
        }
    }

    #[test]
    fn plan_flattens_absolute_and_escaping_paths_without_leaving_samples() {
        let composition = minimal_composition(vec![
            sample("a", "/abs/drums/kick.wav"),
            sample("b", "kept/kick.wav"),
        ]);
        let mut declared_files = StdHashMap::new();
        declared_files.insert("a".to_string(), "/abs/drums/kick.wav".to_string());
        declared_files.insert("b".to_string(), "kept/kick.wav".to_string());
        let project = Project {
            path: PathBuf::from("song.bm1"),
            composition,
            declared_files,
            source: SampleSource::Disk,
        };

        let plan = plan_sample_destinations(&project);
        assert_eq!(plan["a"], "drums/kick.wav");
        assert_eq!(plan["b"], "kept/kick.wav");
        for dest in plan.values() {
            assert!(!dest.starts_with('/'));
            assert!(!dest.contains(".."));
        }
    }

    #[test]
    fn plan_disambiguates_colliding_destinations() {
        let composition = minimal_composition(vec![
            sample("a", "/one/drums/kick.wav"),
            sample("b", "/two/drums/kick.wav"),
        ]);
        let mut declared_files = StdHashMap::new();
        declared_files.insert("a".to_string(), "/one/drums/kick.wav".to_string());
        declared_files.insert("b".to_string(), "/two/drums/kick.wav".to_string());
        let project = Project {
            path: PathBuf::from("song.bm1"),
            composition,
            declared_files,
            source: SampleSource::Disk,
        };

        let plan = plan_sample_destinations(&project);
        let mut values: Vec<&String> = plan.values().collect();
        values.sort();
        assert_ne!(values[0], values[1]);
    }
}
