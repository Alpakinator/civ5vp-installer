//! The Core-facing side: a [`ToolchainRunner`] backed by the Toolchain Cache.
//!
//! Ticket 05 owns everything up to "the Toolchain exists"; ticket 06 owns turning it into a
//! DLL. This type is where those two meet: a build bootstraps the Toolchain first (instant
//! once the cache is populated), then drives the compile through [`DllBuild`].

use std::path::PathBuf;

use civ5vp_core::{BoundaryError, BuildRequest, ProgressReporter, ToolchainRunner};

use crate::bootstrap::ToolchainBootstrap;
use crate::build::{DllBuild, ProcessInvoker};
use crate::cache::{Toolchain, ToolchainCache};

/// A [`ToolchainRunner`] backed by the Toolchain Cache.
pub struct BootstrappedToolchain {
    cache: ToolchainCache,
}

impl BootstrappedToolchain {
    /// `cache_root` is the Toolchain Cache's directory inside the App Data Store.
    pub fn new(cache_root: PathBuf) -> Self {
        Self {
            cache: ToolchainCache::new(cache_root),
        }
    }

    /// Acquire the Toolchain, downloading and extracting it if the cache is empty.
    ///
    /// The first step of every build; every later build finds the cache populated and gets
    /// the Toolchain back in microseconds.
    pub fn bootstrap(&self, progress: &ProgressReporter) -> Result<Toolchain, BoundaryError> {
        Ok(ToolchainBootstrap::new(self.cache.root().to_path_buf()).ensure(progress)?)
    }
}

impl ToolchainRunner for BootstrappedToolchain {
    /// Make the Toolchain exist, then compile the Built DLL with it.
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        let toolchain = self.bootstrap(progress)?;
        let invoker = ProcessInvoker;
        DllBuild::new(&toolchain, &invoker)
            .run(request, progress)
            .map_err(Into::into)
    }

    fn toolchain_identity(&self) -> String {
        // Available before the bootstrap has run: the identity is a property of what is
        // pinned, not of what happens to be on disk. Ticket 07 folds this into the Build
        // Fingerprint, where it has to be knowable up front.
        self.cache.expected_identity()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use civ5vp_core::{BuildConfiguration, FortyThreeCivs};

    #[test]
    fn the_identity_is_known_before_anything_is_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BootstrappedToolchain::new(dir.path().to_path_buf());

        let identity = runner.toolchain_identity();

        assert!(identity.starts_with("clang-18.1.8"));
        assert!(identity.contains("winsdk-7.0"));
        // Nothing was created just by asking.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// The wiring in front of the compile: a populated cache is reused (no download), and
    /// the build refuses a tree with no project file before any tool is spawned.
    ///
    /// A build against an *empty* cache heads straight for the 2.4 GB bootstrap, so the fast
    /// suite must never call `build_dll` with one; the real end-to-end build lives in
    /// `tests/real_build.rs`, `#[ignore]`d (rule 14).
    #[test]
    fn a_bootstrapped_cache_is_used_and_a_sourceless_tree_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ToolchainCache::new(dir.path().join("toolchain"));
        std::fs::create_dir_all(cache.llvm_root().join("bin")).unwrap();
        std::fs::create_dir_all(cache.sdk_root().join("Include")).unwrap();
        cache.mark_complete().unwrap();
        let runner = BootstrappedToolchain::new(dir.path().join("toolchain"));

        let request = BuildRequest {
            source_root: dir.path().join("empty-sources"),
            forty_three_civs: FortyThreeCivs::Disabled,
            build_configuration: BuildConfiguration::Release,
            version_label: "Release-9.9".to_owned(),
            output_path: dir.path().join("CvGameCore_Expansion2.dll"),
        };
        let error = runner
            .build_dll(&request, &ProgressReporter::silent())
            .unwrap_err();

        assert!(error.message().contains("project file"));
        assert!(!dir.path().join("CvGameCore_Expansion2.dll").exists());
    }
}
