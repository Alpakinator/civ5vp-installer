//! What is already built, and whether it can be trusted — the incremental half of the build.
//!
//! The contract (ticket 06): within a build, only sources that changed since the last build
//! recompile. The rules here are deliberately conservative — when in doubt, rebuild — because
//! a stale object linked into the game is a corrupt install nobody can diagnose, while an
//! unnecessary recompile costs seconds.
//!
//! * Everything an object depends on beyond its own source funnels through the precompiled
//!   header fence: if **any** project header is newer than the PCH, the PCH rebuilds, and a
//!   rebuilt PCH makes every object stale. No per-file dependency tracking — headers change
//!   rarely and a header change honestly invalidates close to everything anyway.
//! * The flags, the toolchain, and the source root are recorded in a manifest file next to
//!   the objects. If any of them differ from the manifest, the whole object directory is
//!   discarded first — objects compiled with other flags or from another tree never mix.
//! * Comparison is by file modification time, strictly newer-than. The Upstream Cache
//!   rewrites the working tree only when the Version actually changes, so mtimes carry
//!   honest information here.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use civ5vp_core::{BuildConfiguration, FortyThreeCivs};

use crate::error::{ToolchainError, io_error};

/// The manifest's file name inside a variant directory.
const MANIFEST: &str = "build-manifest.txt";

/// The per-variant directory name: objects for Release, Debug, and their 43-Civs variants
/// never overwrite one another, so switching a toggle twice costs nothing the second time.
pub fn variant_name(
    configuration: BuildConfiguration,
    forty_three_civs: FortyThreeCivs,
) -> &'static str {
    match (configuration, forty_three_civs) {
        (BuildConfiguration::Release, FortyThreeCivs::Disabled) => "release",
        (BuildConfiguration::Release, FortyThreeCivs::Enabled) => "release-43civs",
        (BuildConfiguration::Debug, FortyThreeCivs::Disabled) => "debug",
        (BuildConfiguration::Debug, FortyThreeCivs::Enabled) => "debug-43civs",
    }
}

/// A file's modification time, or `None` for "does not exist" — which every staleness rule
/// treats as infinitely old.
pub fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// `true` when `input` is strictly newer than `product`, or when either is unreadable —
/// an unreadable input is a build problem that surfaces best by rebuilding.
pub fn newer_than(input: &Path, product: Option<SystemTime>) -> bool {
    let Some(product) = product else {
        return true;
    };
    match mtime(input) {
        Some(input) => input > product,
        None => true,
    }
}

/// The newest modification time of any header under `dirs` (recursively), plus any `extra`
/// files — the input side of the PCH fence.
pub fn newest_header(dirs: &[PathBuf], extra: &[PathBuf]) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let mut fold = |time: Option<SystemTime>| {
        if let Some(time) = time {
            newest = Some(newest.map_or(time, |current| current.max(time)));
        }
    };
    for dir in dirs {
        walk_headers(dir, &mut fold);
    }
    for file in extra {
        fold(mtime(file));
    }
    newest
}

fn walk_headers(dir: &Path, fold: &mut impl FnMut(Option<SystemTime>)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk_headers(&path, fold);
        } else if is_header(&path) {
            fold(mtime(&path));
        }
    }
}

fn is_header(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("h" | "hpp" | "inl")
    )
}

/// Make the variant directory trustworthy for `manifest_content`.
///
/// If the recorded manifest differs — other flags, other toolchain, other source root — every
/// object in the directory is discarded and the new manifest written. Returns `true` when the
/// existing contents were kept.
pub fn ensure_manifest(variant_dir: &Path, manifest_content: &str) -> Result<bool, ToolchainError> {
    let manifest_path = variant_dir.join(MANIFEST);
    if fs::read_to_string(&manifest_path).is_ok_and(|recorded| recorded == manifest_content) {
        return Ok(true);
    }
    if variant_dir.exists() {
        fs::remove_dir_all(variant_dir)
            .map_err(|error| io_error("clear the outdated build directory", variant_dir, &error))?;
    }
    fs::create_dir_all(variant_dir)
        .map_err(|error| io_error("create the build directory", variant_dir, &error))?;
    fs::write(&manifest_path, manifest_content)
        .map_err(|error| io_error("write the build manifest", &manifest_path, &error))?;
    Ok(false)
}

/// Write `content` only when the file does not already hold it, so an unchanged input keeps
/// its mtime and does not ripple a rebuild through everything that depends on it.
pub fn write_if_changed(path: &Path, content: &str) -> Result<(), ToolchainError> {
    if fs::read_to_string(path).is_ok_and(|current| current == content) {
        return Ok(());
    }
    fs::write(path, content).map_err(|error| io_error("write", path, &error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_four_variants_have_distinct_directories() {
        let names: std::collections::BTreeSet<&str> = [
            variant_name(BuildConfiguration::Release, FortyThreeCivs::Disabled),
            variant_name(BuildConfiguration::Release, FortyThreeCivs::Enabled),
            variant_name(BuildConfiguration::Debug, FortyThreeCivs::Disabled),
            variant_name(BuildConfiguration::Debug, FortyThreeCivs::Enabled),
        ]
        .into();
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn a_changed_manifest_discards_the_objects() {
        let dir = tempfile::tempdir().unwrap();
        let variant = dir.path().join("release");

        assert!(!ensure_manifest(&variant, "flags v1").unwrap());
        fs::write(variant.join("CvCity.obj"), b"obj").unwrap();
        assert!(ensure_manifest(&variant, "flags v1").unwrap());
        assert!(variant.join("CvCity.obj").exists());

        assert!(!ensure_manifest(&variant, "flags v2").unwrap());
        assert!(!variant.join("CvCity.obj").exists());
    }

    #[test]
    fn unchanged_content_keeps_the_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("commit_id.inc");
        write_if_changed(&file, "v1").unwrap();
        let first = mtime(&file).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        write_if_changed(&file, "v1").unwrap();
        assert_eq!(mtime(&file).unwrap(), first);

        write_if_changed(&file, "v2").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "v2");
    }

    #[test]
    fn missing_files_count_as_infinitely_old_inputs_and_stale_products() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("a.cpp");
        fs::write(&existing, "x").unwrap();

        // A product that does not exist is stale against any input.
        assert!(newer_than(&existing, None));
        // An input that does not exist forces a rebuild rather than hiding a problem.
        assert!(newer_than(&dir.path().join("gone.cpp"), mtime(&existing)));
    }

    #[test]
    fn newest_header_sees_nested_headers_and_extra_files() {
        let dir = tempfile::tempdir().unwrap();
        let include = dir.path().join("include");
        fs::create_dir_all(include.join("nested")).unwrap();
        fs::write(include.join("a.h"), "x").unwrap();
        fs::write(include.join("nested/b.inl"), "x").unwrap();
        fs::write(include.join("ignored.cpp"), "x").unwrap();
        let extra = dir.path().join("commit_id.inc");
        fs::write(&extra, "x").unwrap();

        let newest =
            newest_header(std::slice::from_ref(&include), std::slice::from_ref(&extra)).unwrap();

        let expected = [
            mtime(&include.join("a.h")).unwrap(),
            mtime(&include.join("nested/b.inl")).unwrap(),
            mtime(&extra).unwrap(),
        ]
        .into_iter()
        .max()
        .unwrap();
        assert_eq!(newest, expected);
        // The .cpp file's mtime is not part of the header fence.
        assert!(newest_header(&[include], &[]).is_some());
    }
}
