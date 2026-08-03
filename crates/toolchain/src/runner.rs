//! The Core-facing side: a [`ToolchainRunner`] backed by the Toolchain Cache.
//!
//! Ticket 05 owns everything up to "the Toolchain exists"; ticket 06 owns turning it into a
//! DLL. This type is where those two meet, so the Core can be wired to the real bootstrap now
//! and gain a compiler later without the seam moving.

use std::path::PathBuf;

use civ5vp_core::{BoundaryError, BuildRequest, ProgressReporter, Stage, ToolchainRunner};

use crate::bootstrap::ToolchainBootstrap;
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
    /// Separate from [`ToolchainRunner::build_dll`] on purpose — see that method. Ticket 06
    /// calls this as the first step of a real build, at which point the two fold together.
    pub fn bootstrap(&self, progress: &ProgressReporter) -> Result<Toolchain, BoundaryError> {
        Ok(ToolchainBootstrap::new(self.cache.root().to_path_buf()).ensure(progress)?)
    }
}

impl ToolchainRunner for BootstrappedToolchain {
    /// Make the Toolchain exist, then report that compiling it into a DLL is ticket 06's job.
    ///
    /// A Toolchain that is already cached is reported and nothing happens. A Toolchain that is
    /// *not* cached is **not** downloaded: making a user wait for 2.4 GB and then telling them
    /// the compiler is not implemented would be the worst of both. Ticket 06 replaces this
    /// method body, at which point the bootstrap becomes the first thing a real build does.
    fn build_dll(
        &self,
        _request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        let detail = match self.cache.installed() {
            Some(toolchain) => {
                progress.report(
                    Stage::Build,
                    format!("Build tools ready at {}.", toolchain.sdk_root().display()),
                );
                format!(
                    "toolchain {} is bootstrapped at {}; compilation is not implemented \
                     (ticket 06)",
                    toolchain.identity(),
                    toolchain.llvm_root().display()
                )
            }
            None => format!(
                "no toolchain in {}; not downloading 2.4 GB for a build that cannot run yet \
                 (ticket 06)",
                self.cache.root().display()
            ),
        };

        // A typed error rather than a stub DLL: the Core checks that the Built DLL appeared
        // and would otherwise report a missing file, which tells a user nothing.
        Err(BoundaryError::new(
            "This version of the installer can set up the build tools but cannot compile the \
             mod's DLL yet.",
            detail,
        ))
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
    use civ5vp_core::FortyThreeCivs;

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

    /// The compile step is not implemented, and says so in a sentence rather than by
    /// producing something that looks like a DLL — and without downloading 2.4 GB first.
    #[test]
    fn building_reports_plainly_that_compilation_is_not_implemented_yet() {
        let dir = tempfile::tempdir().unwrap();
        let runner = BootstrappedToolchain::new(dir.path().join("toolchain"));

        let request = BuildRequest {
            source_root: dir.path().to_path_buf(),
            forty_three_civs: FortyThreeCivs::Disabled,
            output_path: dir.path().join("CvGameCore_Expansion2.dll"),
        };
        let error = runner
            .build_dll(&request, &ProgressReporter::silent())
            .unwrap_err();

        assert!(error.message().contains("cannot compile"));
        assert!(error.detail().contains("no toolchain in"));
        // Nothing was downloaded and nothing was created.
        assert!(!dir.path().join("toolchain").exists());
        assert!(!dir.path().join("CvGameCore_Expansion2.dll").exists());
    }
}
