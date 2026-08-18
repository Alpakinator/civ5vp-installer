//! Proving the extraction produced a usable SDK, before anything tries to compile against it.
//!
//! `docs/pinned-artifacts.md` §4: a bootstrap is only complete when `windows.h`, `stdio.h`,
//! `iostream`, `kernel32.lib` and `msvcrt.lib` all resolve under the toolchain root — two SDK
//! headers, the CRT's C and C++ headers, and one import library from each half. Between them
//! they touch every one of the four ISO members and the fix-ups, which is why that list and
//! not a longer one.
//!
//! §4 used to name `DriverSpecs.h` as a sixth. It was removed because it could not fail: fix-up
//! 6 wrote a stub by that name into every include root immediately before this check looked for
//! it, so the check reported success against the very file that was making `windows.h`
//! impossible to include. A verification that passes *because* of a bug is worse than none.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{ToolchainError, io_error};
use crate::pinned::VERIFICATION_NAMES;

/// Header and import-library counts for a known-good extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Baseline {
    pub headers: usize,
    pub libs: usize,
}

/// The committed comparison baseline.
///
/// **Measured by this implementation, not verified against the docker image.**
/// `docs/pinned-artifacts.md` §4 asks for the counts from a real docker build; nobody here has
/// run that container. These are what `real_bootstrap.rs` measured extracting the pinned image
/// on 2026-08-03 — 2033 headers and 928 import libraries, with all six of
/// [`VERIFICATION_NAMES`] resolving.
///
/// So this is a **regression guard on our own extraction**, which is worth having: it catches
/// a reader or fix-up change that silently drops files. It is *not* the cross-check against a
/// known-good build that the document is asking for. When someone runs the reference
/// container, replace these and say so here — if they differ, ours is the one that is wrong.
///
/// The header count was 2033 until fix-up 6 stopped stubbing headers the SDK already ships.
/// The difference is exactly the six stubs it used to write, and dropping them is what made
/// `windows.h` includable at all — so the lower number is the correct one.
pub const REFERENCE_BASELINE: Option<Baseline> = Some(Baseline {
    headers: 2027,
    libs: 928,
});

/// What an extraction turned out to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionReport {
    /// Distinct header files: real files (not case symlinks) under an `Include` root.
    pub headers: usize,
    /// Distinct import libraries: real `.lib` files (not case symlinks) under a `Lib` root.
    pub libs: usize,
    /// Where each of [`VERIFICATION_NAMES`] was found, in the document's order.
    pub resolved: Vec<(String, PathBuf)>,
    /// Which of them were not found at all.
    pub missing: Vec<String>,
}

impl ExtractionReport {
    /// Whether every name `docs/pinned-artifacts.md` §4 lists resolved.
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }

    /// A line for the log, listing what was found and where.
    pub fn summary(&self) -> String {
        let found = self
            .resolved
            .iter()
            .map(|(name, path)| format!("{name}={}", path.display()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "headers={}, libs={}, resolved: {found}, missing: {:?}",
            self.headers, self.libs, self.missing
        )
    }
}

/// Walk the extracted tree and answer §4's question.
pub fn verify_extraction(sdk_root: &Path) -> Result<ExtractionReport, ToolchainError> {
    // One walk. Symlinks are counted as neither headers nor libs — the fix-ups add several
    // spellings per file, and counting those would make the totals depend on how many
    // spellings the SDK happened to use rather than on what it shipped.
    let mut headers = 0usize;
    let mut libs = 0usize;
    let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
    let wanted = VERIFICATION_NAMES;

    let mut queue = vec![sdk_root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| io_error("list the toolchain folder", &directory, &error))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| io_error("list the toolchain folder", &directory, &error))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| io_error("inspect a toolchain file", &path, &error))?;

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            // A verification name counts whichever way it resolves — a case symlink added by
            // fix-up 1 is exactly as good as a real file, because the compiler will open it.
            //
            // Matched without regard to case, and keyed by the spelling §4 uses so the caller
            // can look the answer up. The SDK's own spellings are inconsistent — `Kernel32.Lib`
            // beside `msvcrt.lib` — and on Windows no fix-up runs to add a lowercase alias,
            // because NTFS never needed one. An exact match therefore reported a file the
            // compiler opens perfectly well as missing, and sent the player to clear a data
            // folder that was never the problem, forever.
            if !metadata.is_dir()
                && let Some(canonical) = wanted.iter().find(|want| want.eq_ignore_ascii_case(name))
                && !found.contains_key(*canonical)
            {
                found.insert((*canonical).to_string(), path.clone());
            }

            if metadata.is_dir() {
                queue.push(path);
            } else if metadata.is_file() {
                if under_directory_named(&directory, sdk_root, "include") && is_header(name) {
                    headers += 1;
                } else if under_directory_named(&directory, sdk_root, "lib") && is_lib(name) {
                    libs += 1;
                }
            }
        }
    }

    let resolved: Vec<(String, PathBuf)> = wanted
        .iter()
        .filter_map(|name| {
            found
                .get(*name)
                .map(|path| ((*name).to_string(), path.clone()))
        })
        .collect();
    let missing: Vec<String> = wanted
        .iter()
        .filter(|name| !found.contains_key(**name))
        .map(|name| (*name).to_string())
        .collect();

    Ok(ExtractionReport {
        headers,
        libs,
        resolved,
        missing,
    })
}

/// Turn a report into the error a failed bootstrap must not get past.
pub fn require_complete(report: &ExtractionReport, sdk_root: &Path) -> Result<(), ToolchainError> {
    if report.is_complete() {
        return Ok(());
    }
    Err(ToolchainError::new(
        "The Windows SDK did not unpack completely. Clear the installer's data folder and try \
         again — the files will be downloaded and unpacked from scratch.",
        format!(
            "verification of {} failed; {}",
            sdk_root.display(),
            report.summary()
        ),
    ))
}

/// Whether any directory between `root` and `directory` is called `name`, ignoring case.
///
/// Not just the first component: where the SDK and CRT roots end up depends on each MSI's
/// `Directory` table, and the CRT's puts its headers several levels down. Matching at any
/// depth means the counts mean "headers the toolchain ships" regardless of that layout —
/// which is what makes them comparable between runs. Case-insensitive because fix-up 3 makes
/// both spellings exist.
fn under_directory_named(directory: &Path, root: &Path, name: &str) -> bool {
    let Ok(relative) = directory.strip_prefix(root) else {
        return false;
    };
    relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|segment| segment.eq_ignore_ascii_case(name))
}

/// The SDK ships `.h`, the CRT ships `.h`, `.hpp`, `.inl` and extension-less C++ headers
/// (`iostream`, `vector`). All four count.
fn is_header(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, extension)) => ["h", "hpp", "hxx", "inl", "idl"]
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known)),
        None => true,
    }
}

fn is_lib(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("lib"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A tree shaped like a finished extraction, small enough to write by hand.
    fn extracted_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Include/gl")).unwrap();
        fs::create_dir_all(root.join("VC/include")).unwrap();
        fs::create_dir_all(root.join("Lib")).unwrap();
        fs::create_dir_all(root.join("VC/lib")).unwrap();
        for (path, content) in [
            ("Include/windows.h", "#pragma once\n"),
            ("Include/windef.h", "#pragma once\n"),
            ("Include/DriverSpecs.h", "/* stub */\n"),
            ("Include/gl/gl.h", "/* gl */\n"),
            ("VC/include/stdio.h", "/* crt */\n"),
            ("VC/include/iostream", "/* crt */\n"),
            ("Lib/kernel32.lib", "lib"),
            ("Lib/gdi32.lib", "lib"),
            ("VC/lib/msvcrt.lib", "lib"),
        ] {
            fs::write(root.join(path), content).unwrap();
        }
        dir
    }

    #[test]
    fn every_name_the_document_lists_is_looked_for() {
        let dir = extracted_tree();
        // `stdio.h`, `iostream` and `msvcrt.lib` live under VC/, which this walk reaches.
        let report = verify_extraction(dir.path()).unwrap();

        assert!(report.is_complete(), "{}", report.summary());
        assert_eq!(report.resolved.len(), VERIFICATION_NAMES.len());
        assert!(report.missing.is_empty());
    }

    /// The SDK spells its own files inconsistently — `Kernel32.Lib` beside `msvcrt.lib` —
    /// and on Windows no fix-up runs to add a lowercase alias, because NTFS never needed one.
    /// Matching exactly reported a file the compiler opens perfectly well as missing, and the
    /// sentence it produced sent the player to clear a data folder that was never at fault.
    #[test]
    fn a_verification_name_resolves_whatever_case_the_sdk_shipped_it_in() {
        let dir = extracted_tree();
        let lib = dir.path().join("Lib");
        fs::rename(lib.join("kernel32.lib"), lib.join("Kernel32.Lib")).unwrap();

        let report = verify_extraction(dir.path()).unwrap();

        assert!(report.is_complete(), "{}", report.summary());
        // Reported under the spelling the document uses, whatever is on disk, because that is
        // the name every caller looks the answer up by.
        assert!(
            report
                .resolved
                .iter()
                .any(|(name, path)| name == "kernel32.lib" && path.ends_with("Kernel32.Lib")),
            "{}",
            report.summary()
        );
    }

    #[test]
    fn a_missing_name_is_named_and_stops_the_bootstrap() {
        let dir = extracted_tree();
        fs::remove_file(dir.path().join("Include/windows.h")).unwrap();

        let report = verify_extraction(dir.path()).unwrap();

        assert!(!report.is_complete());
        assert_eq!(report.missing, vec!["windows.h".to_string()]);
        let error = require_complete(&report, dir.path()).unwrap_err();
        // The name goes in the log, and the player gets a sentence instead.
        assert!(error.detail().contains("windows.h"));
        assert!(!error.message().contains("windows.h"));
    }

    /// The counts are the comparison baseline, so they must mean the same thing every run —
    /// which means not counting the case symlinks the fix-ups add.
    #[cfg(unix)]
    #[test]
    fn case_symlinks_do_not_inflate_the_counts() {
        let dir = extracted_tree();
        let before = verify_extraction(dir.path()).unwrap();

        std::os::unix::fs::symlink("windows.h", dir.path().join("Include/Windows.h")).unwrap();
        std::os::unix::fs::symlink("kernel32.lib", dir.path().join("Lib/Kernel32.Lib")).unwrap();
        let after = verify_extraction(dir.path()).unwrap();

        assert_eq!(before.headers, after.headers);
        assert_eq!(before.libs, after.libs);
    }

    /// …but a name that only resolves through a symlink still counts as resolving, because
    /// that is exactly what the compiler will do with it.
    #[cfg(unix)]
    #[test]
    fn a_verification_name_reached_through_a_symlink_still_resolves() {
        let dir = extracted_tree();
        fs::rename(
            dir.path().join("Include/DriverSpecs.h"),
            dir.path().join("Include/driverspecs.h"),
        )
        .unwrap();
        std::os::unix::fs::symlink("driverspecs.h", dir.path().join("Include/DriverSpecs.h"))
            .unwrap();

        assert!(verify_extraction(dir.path()).unwrap().is_complete());
    }

    #[test]
    fn headers_and_libs_are_counted_separately_and_only_where_they_belong() {
        let dir = extracted_tree();
        // Neither of these is under an `include` or a `lib` directory, so neither counts.
        fs::write(dir.path().join("readme.h"), "x").unwrap();
        fs::write(dir.path().join("stray.lib"), "x").unwrap();
        fs::create_dir_all(dir.path().join("Bin")).unwrap();
        fs::write(dir.path().join("Bin/rc.exe"), "x").unwrap();

        let report = verify_extraction(dir.path()).unwrap();

        // Include/: windows.h, windef.h, DriverSpecs.h, gl/gl.h — plus the CRT's two, which
        // live at VC/include/ and count all the same.
        assert_eq!(report.headers, 6);
        // Lib/kernel32.lib, Lib/gdi32.lib, VC/lib/msvcrt.lib.
        assert_eq!(report.libs, 3);
    }

    #[test]
    fn extension_less_cpp_headers_count_as_headers() {
        assert!(is_header("iostream"));
        assert!(is_header("windows.h"));
        assert!(is_header("pshpack1.h"));
        assert!(!is_header("kernel32.lib"));
        assert!(!is_header("setup.exe"));
        assert!(is_lib("Kernel32.Lib"));
        assert!(!is_lib("windows.h"));
    }
}
