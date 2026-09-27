//! Everything that touches the file system for a composition: loading a
//! `.bm1`, resolving its sample paths relative to it, checking that the
//! referenced samples exist, and bundling a composition with its samples
//! into a portable `.bmz` package (see [`package`]).
//!
//! This crate never prints; it returns values and structured errors so each
//! application decides how to present them.

pub mod package;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bm_format::{Composition, FormatError};
use bm_wav::WavError;
use thiserror::Error;

pub use package::{PackageError, PackageReport, load_bmz, write_package};

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("could not read file '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Format(#[from] FormatError),

    #[error("could not read package '{path}': {source}")]
    Zip {
        path: String,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("'{path}' is not a bit-music package (it has no .bm1 file inside)")]
    NoComposition { path: String },
}

/// Where a project's bytes live: the real file system, or an in-memory
/// `.bmz` archive, already fully decompressed at load time and keyed by the
/// zip entry name each `sample.file` was resolved to (see [`load_bmz`]).
#[derive(Debug, Clone)]
pub enum SampleSource {
    Disk,
    Archive(Arc<HashMap<String, Vec<u8>>>),
}

impl SampleSource {
    /// Reads the bytes of `file` (a `Project.composition.samples[].file`
    /// value, already resolved by [`load`] or [`load_bmz`]).
    pub fn read(&self, file: &str) -> std::io::Result<Vec<u8>> {
        match self {
            SampleSource::Disk => std::fs::read(file),
            SampleSource::Archive(entries) => entries.get(file).cloned().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("'{file}' not found in the package"),
                )
            }),
        }
    }
}

/// A loaded, validated composition together with where it came from.
#[derive(Debug, Clone)]
pub struct Project {
    /// Path of the file this project was opened from: a `.bm1` or a
    /// `.bmz`.
    pub path: PathBuf,
    /// The composition. Every `sample.file` is already resolved to a key
    /// usable with `source`: an absolute path (as written, or relative to
    /// the `.bm1`'s directory) for [`SampleSource::Disk`], or a zip entry
    /// name for [`SampleSource::Archive`].
    pub composition: Composition,
    /// Each sample's `file` exactly as written in the `.bm1`, by sample id
    /// (for showing what is stored, as opposed to the resolved path, and
    /// for planning a `.bmz`'s folder structure).
    pub declared_files: HashMap<String, String>,
    /// Where to read a sample's bytes from.
    pub source: SampleSource,
}

/// Result of checking one sample file.
#[derive(Debug)]
pub struct SampleReport {
    pub sample_id: String,
    /// The (already resolved) path or archive entry that was checked.
    pub file: String,
    /// `Ok` if the file exists and is a well-formed `.wav`.
    pub outcome: Result<(), WavError>,
}

/// Loads a project from `path`: a `.bm1` through [`load`], or a `.bmz`
/// package through [`load_bmz`], dispatched by extension (case-insensitive).
pub fn load_path(path: &Path) -> Result<Project, ProjectError> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bmz")) {
        let bytes = std::fs::read(path).map_err(|source| ProjectError::Io {
            path: path.display().to_string(),
            source,
        })?;
        package::load_bmz(&bytes, path.to_path_buf())
    } else {
        load(path)
    }
}

/// Loads a composition from a `.bm1` (JSON) file.
///
/// - `sample.file` can be an absolute or relative path. If relative
///   (including just a bare file name, no directory), it is resolved taking
///   the directory the `.bm1` itself lives in as the starting point — so a
///   composition can bring its samples along in the same folder (or a
///   relative one) without depending on the working directory the caller
///   runs from.
pub fn load(path: &Path) -> Result<Project, ProjectError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ProjectError::Io {
        path: path.display().to_string(),
        source,
    })?;

    let mut composition = bm_format::parse_composition(&raw)?;

    let declared_files = composition
        .samples
        .iter()
        .map(|s| (s.id.clone(), s.file.clone()))
        .collect();

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    for sample in &mut composition.samples {
        sample.file = resolve_sample_path(base_dir, &sample.file);
    }

    Ok(Project {
        path: path.to_path_buf(),
        composition,
        declared_files,
        source: SampleSource::Disk,
    })
}

/// Checks that every sample `project.composition` references is present and
/// a well-formed `.wav`, reading from `project.source` (paths must already
/// be resolved, as after [`load`] or [`load_bmz`]).
pub fn check_samples(project: &Project) -> Vec<SampleReport> {
    project
        .composition
        .samples
        .iter()
        .map(|sample| {
            let outcome = match &project.source {
                SampleSource::Disk => bm_wav::check_wav(Path::new(&sample.file)),
                SampleSource::Archive(entries) => match entries.get(&sample.file) {
                    Some(bytes) => bm_wav::check_wav_bytes(bytes, &sample.file),
                    None => Err(bm_wav::missing(&sample.file)),
                },
            };
            SampleReport {
                sample_id: sample.id.clone(),
                file: sample.file.clone(),
                outcome,
            }
        })
        .collect()
}

/// Resolves a sample's path: absolute as-is, relative against `base_dir`
/// (the `.bm1`'s directory).
fn resolve_sample_path(base_dir: &Path, file: &str) -> String {
    let file_path = Path::new(file);
    if file_path.is_absolute() {
        file.to_string()
    } else {
        base_dir.join(file_path).to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_path_against_base_dir() {
        let base = Path::new("/songs/demo");
        assert_eq!(
            resolve_sample_path(base, "kick.wav"),
            Path::new("/songs/demo/kick.wav").to_string_lossy()
        );
        assert_eq!(
            resolve_sample_path(base, "../samples/kick.wav"),
            Path::new("/songs/demo/../samples/kick.wav").to_string_lossy()
        );
    }

    #[test]
    fn keeps_absolute_path_untouched() {
        let base = Path::new("/songs/demo");
        #[cfg(unix)]
        assert_eq!(
            resolve_sample_path(base, "/etc/samples/kick.wav"),
            "/etc/samples/kick.wav"
        );
    }

    #[test]
    fn load_keeps_the_declared_file_next_to_the_resolved_one() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/songs/song1.bm1");
        let project = load(&path).unwrap();
        let kick = project.composition.samples.iter().find(|s| s.id == "kick").unwrap();
        let declared = &project.declared_files["kick"];
        assert!(!Path::new(declared).is_absolute());
        assert_ne!(&kick.file, declared);
        assert!(kick.file.ends_with(declared.trim_start_matches("./")));
    }

    #[test]
    fn load_reports_missing_file_as_io_error() {
        let err = load(Path::new("/definitely/not/here.bm1")).unwrap_err();
        assert!(matches!(err, ProjectError::Io { .. }));
    }

    #[test]
    fn check_samples_reports_each_sample_with_its_own_outcome() {
        let dir = std::env::temp_dir().join(format!("bm-project-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        bm_wav::write_wav(&dir.join("good.wav"), &[0.0, 0.5], 44100).unwrap();

        let song = r#"{
            "metadata": { "version": "1.0", "title": "t", "bpm": 120 },
            "samples": [ { "id": "good", "file": "good.wav" },
                         { "id": "gone", "file": "gone.wav" } ],
            "patterns": [ { "id": "p", "sample": "good", "steps": ["C4", null, null, null] } ],
            "arrangement": { "tracks": [ { "id": "t", "sequence": ["p"] } ] }
        }"#;
        std::fs::write(dir.join("song.bm1"), song).unwrap();

        let project = load(&dir.join("song.bm1")).unwrap();
        let reports = check_samples(&project);
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(reports.len(), 2);
        assert!(reports[0].outcome.is_ok());
        assert!(reports[1].outcome.is_err());
    }

    #[test]
    fn load_reads_from_disk() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/songs/song1.bm1");
        let project = load(&path).unwrap();
        assert!(matches!(project.source, SampleSource::Disk));
    }

    #[test]
    fn load_path_dispatches_by_extension() {
        let bm1 = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/songs/song1.bm1");
        let via_load_path = load_path(&bm1).unwrap();
        assert!(matches!(via_load_path.source, SampleSource::Disk));
        assert_eq!(via_load_path.composition.metadata.title, "Demo");
    }
}
