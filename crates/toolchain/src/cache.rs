//! The Toolchain Cache: where the bootstrapped Toolchain lives, and how "is it already
//! there?" is answered.
//!
//! The answer has to be all-or-nothing. A bootstrap that was killed halfway leaves a
//! plausible-looking tree of headers behind, and a build started against it would fail in a
//! way nobody could read. So completeness is a single marker file written last, holding the
//! Toolchain identity; until it exists the cache counts as absent and its contents are
//! discarded on the next attempt — an interrupted bootstrap self-repairs on retry.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{ToolchainError, io_error};
use crate::pinned::{LLVM_TARGET_TRIPLE, LLVM_VERSION};

/// Name of the marker. Leading dot so it sorts away from the toolchain's own directories.
const MARKER: &str = ".toolchain-complete";

/// The version of this crate's own layout. Bumping it invalidates every existing cache — the
/// alternative is a user whose half-right toolchain from an older installer silently produces
/// a different DLL.
const LAYOUT_VERSION: u32 = 1;

/// A bootstrapped, complete Toolchain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    identity: String,
    llvm_root: PathBuf,
    sdk_root: PathBuf,
}

impl Toolchain {
    /// A stable identifier for this Toolchain, e.g.
    /// `clang-18.1.8+i386-pc-windows-msvc+winsdk-7.0+layout-1`.
    ///
    /// `CONTEXT.md` makes the Toolchain version part of the Build Fingerprint, and
    /// `docs/pinned-artifacts.md` §2 says the clang major version is part of the Toolchain's
    /// identity rather than an incidental detail. This string is what carries both.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Root of the portable LLVM: `bin/clang-cl`, `bin/lld-link`, `lib/clang/18/include`.
    pub fn llvm_root(&self) -> &Path {
        &self.llvm_root
    }

    /// Root of the extracted Windows SDK 7.0 and VC9 CRT.
    ///
    /// Not itself an include path: the MSIs place their contents under the path Windows would
    /// have installed to, so the real headers are several levels down. Use
    /// [`Toolchain::include_dirs`] and [`Toolchain::lib_dirs`] rather than joining onto this.
    pub fn sdk_root(&self) -> &Path {
        &self.sdk_root
    }

    /// Every `Include` directory in the extracted SDK, sorted — what the build puts on the
    /// compiler's include path.
    pub fn include_dirs(&self) -> Result<Vec<PathBuf>, ToolchainError> {
        Ok(crate::sdk_layout::find(&self.sdk_root)?.include)
    }

    /// Every `Lib` directory in the extracted SDK, sorted — what the build puts on the
    /// linker's library path.
    pub fn lib_dirs(&self) -> Result<Vec<PathBuf>, ToolchainError> {
        Ok(crate::sdk_layout::find(&self.sdk_root)?.lib)
    }

    /// The compiler driver the build invokes.
    pub fn clang_path(&self) -> PathBuf {
        crate::tarball::clang_path(&self.llvm_root)
    }

    /// The linker the build invokes.
    pub fn lld_link_path(&self) -> PathBuf {
        crate::tarball::lld_link_path(&self.llvm_root)
    }
}

/// One directory in the App Data Store, holding everything Toolchain Bootstrap produces.
///
/// ```text
/// <root>/downloads/            verified, reusable: the ISO and the LLVM tarball
/// <root>/llvm-18.1.8/          the portable compiler
/// <root>/winsdk-7.0/           the extracted SDK and VC9 CRT
/// <root>/staging/              scratch, deleted when the bootstrap finishes
/// <root>/.toolchain-complete   written last; its contents are the Toolchain identity
/// ```
#[derive(Debug, Clone)]
pub struct ToolchainCache {
    root: PathBuf,
}

impl ToolchainCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Kept across bootstraps: a verified download never needs fetching twice, even if the
    /// extraction that followed it failed.
    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    pub fn llvm_root(&self) -> PathBuf {
        self.root.join(format!("llvm-{LLVM_VERSION}"))
    }

    pub fn sdk_root(&self) -> PathBuf {
        self.root.join("winsdk-7.0")
    }

    /// Scratch space for CABs on their way out of the ISO. Emptied at both ends of a
    /// bootstrap, so an interrupted run cannot leave hundreds of megabytes behind.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// What a complete cache would be identified as.
    pub fn expected_identity(&self) -> String {
        format!("clang-{LLVM_VERSION}+{LLVM_TARGET_TRIPLE}+winsdk-7.0+layout-{LAYOUT_VERSION}")
    }

    /// The Toolchain already in this cache, if there is a complete one.
    ///
    /// Returns `None` for a cache that is absent, half-populated, or was written by an
    /// installer whose Toolchain identity differs from this one's — all three mean the same
    /// thing to the caller: bootstrap it.
    pub fn installed(&self) -> Option<Toolchain> {
        let recorded = fs::read_to_string(self.root.join(MARKER)).ok()?;
        let recorded = recorded.trim();
        if recorded != self.expected_identity() {
            return None;
        }
        let llvm_root = self.llvm_root();
        let sdk_root = self.sdk_root();
        // The marker is only trusted as far as the directories it claims exist.
        if !llvm_root.is_dir() || !sdk_root.is_dir() {
            return None;
        }
        Some(Toolchain {
            identity: recorded.to_string(),
            llvm_root,
            sdk_root,
        })
    }

    /// Throw away everything except the verified downloads, so the next attempt starts from a
    /// known-empty tree rather than from whatever an interrupted run left.
    pub fn discard_partial_state(&self) -> Result<(), ToolchainError> {
        let marker = self.root.join(MARKER);
        if marker.exists() {
            fs::remove_file(&marker)
                .map_err(|error| io_error("clear the toolchain marker", &marker, &error))?;
        }
        for directory in [self.llvm_root(), self.sdk_root(), self.staging_dir()] {
            if directory.exists() {
                fs::remove_dir_all(&directory).map_err(|error| {
                    io_error("clear a partly-installed toolchain", &directory, &error)
                })?;
            }
        }
        Ok(())
    }

    /// Declare the cache complete. Called once, last, after verification has passed.
    pub fn mark_complete(&self) -> Result<Toolchain, ToolchainError> {
        let identity = self.expected_identity();
        let marker = self.root.join(MARKER);
        // Written through a temporary file so a crash mid-write cannot leave a marker that
        // parses as a different, wrong identity.
        let temporary = self.root.join(format!("{MARKER}.new"));
        fs::write(&temporary, &identity)
            .map_err(|error| io_error("write the toolchain marker", &temporary, &error))?;
        fs::rename(&temporary, &marker)
            .map_err(|error| io_error("write the toolchain marker", &marker, &error))?;
        Ok(Toolchain {
            identity,
            llvm_root: self.llvm_root(),
            sdk_root: self.sdk_root(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn populated_cache(dir: &Path) -> ToolchainCache {
        let cache = ToolchainCache::new(dir.to_path_buf());
        fs::create_dir_all(cache.llvm_root().join("bin")).unwrap();
        fs::create_dir_all(cache.sdk_root().join("Include")).unwrap();
        cache
    }

    #[test]
    fn an_empty_cache_has_no_toolchain() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            ToolchainCache::new(dir.path().to_path_buf())
                .installed()
                .is_none()
        );
    }

    /// Bootstrap runs once; later builds detect the populated cache.
    #[test]
    fn a_marked_cache_reports_its_toolchain() {
        let dir = tempfile::tempdir().unwrap();
        let cache = populated_cache(dir.path());

        let marked = cache.mark_complete().unwrap();
        let found = cache.installed().unwrap();

        assert_eq!(found, marked);
        assert!(
            found
                .identity()
                .starts_with("clang-18.1.8+i386-pc-windows-msvc")
        );
    }

    /// A tree of files with no marker is not a Toolchain, however complete it looks.
    #[test]
    fn a_populated_but_unmarked_cache_counts_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cache = populated_cache(dir.path());
        fs::write(cache.sdk_root().join("Include/windows.h"), "#pragma once\n").unwrap();

        assert!(cache.installed().is_none());
    }

    #[test]
    fn a_marker_whose_directories_are_gone_counts_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cache = populated_cache(dir.path());
        cache.mark_complete().unwrap();
        fs::remove_dir_all(cache.sdk_root()).unwrap();

        assert!(cache.installed().is_none());
    }

    #[test]
    fn a_marker_from_a_different_toolchain_version_counts_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cache = populated_cache(dir.path());
        fs::write(dir.path().join(MARKER), "clang-17.0.6+winsdk-7.0+layout-1").unwrap();

        assert!(cache.installed().is_none());
    }

    /// Self-repair: the half-built tree goes, the expensive verified downloads stay.
    #[test]
    fn discarding_partial_state_keeps_the_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let cache = populated_cache(dir.path());
        fs::create_dir_all(cache.downloads_dir()).unwrap();
        fs::write(cache.downloads_dir().join("GRMSDK_EN_DVD.iso"), b"iso").unwrap();
        fs::create_dir_all(cache.staging_dir()).unwrap();
        fs::write(cache.staging_dir().join("staged-cab1.cab"), b"cab").unwrap();
        cache.mark_complete().unwrap();

        cache.discard_partial_state().unwrap();

        assert!(cache.installed().is_none());
        assert!(!cache.sdk_root().exists());
        assert!(!cache.llvm_root().exists());
        assert!(!cache.staging_dir().exists());
        assert!(cache.downloads_dir().join("GRMSDK_EN_DVD.iso").exists());
    }

    #[test]
    fn discarding_partial_state_on_an_empty_cache_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        ToolchainCache::new(dir.path().to_path_buf())
            .discard_partial_state()
            .unwrap();
    }
}
