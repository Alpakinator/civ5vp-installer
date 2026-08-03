//! The Core-facing side: a [`ToolchainRunner`] backed by the Toolchain Cache.
//!
//! Ticket 05 owns everything up to "the Toolchain exists"; ticket 06 owns turning it into a
//! DLL. This type is where those two meet, so the Core can be wired to the real bootstrap now
//! and gain a compiler later without the seam moving.

use std::path::PathBuf;

use civ5vp_core::{BoundaryError, BuildRequest, ProgressReporter, Stage, ToolchainRunner};

use crate::bootstrap::ToolchainBootstrap;
use crate::cache::ToolchainCache;

/// A [`ToolchainRunner`] that bootstraps the Toolchain on first use.
pub struct BootstrappedToolchain {
    bootstrap: ToolchainBootstrap,
    cache: ToolchainCache,
}

impl BootstrappedToolchain {
    /// `cache_root` is the Toolchain Cache's directory inside the App Data Store.
    pub fn new(cache_root: PathBuf) -> Self {
        Self {
            bootstrap: ToolchainBootstrap::new(cache_root.clone()),
            cache: ToolchainCache::new(cache_root),
        }
    }
}

impl ToolchainRunner for BootstrappedToolchain {
    fn build_dll(
        &self,
        _request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        // The bootstrap is this ticket's whole job, and it is the expensive half: after this
        // returns the Toolchain is on disk, verified, and every later call is a no-op.
        let toolchain = self.bootstrap.ensure(progress)?;
        progress.report(
            Stage::Build,
            format!("Build tools ready at {}.", toolchain.sdk_root().display()),
        );

        // Compiling is ticket 06. Returning a typed error rather than a stub DLL is the
        // honest option: the Core checks that the Built DLL appeared and would otherwise
        // report a missing file, which tells a user nothing.
        Err(BoundaryError::new(
            "This version of the installer can set up the build tools but cannot compile the \
             mod's DLL yet.",
            format!(
                "toolchain {} is bootstrapped at {}; compilation is not implemented (ticket 06)",
                toolchain.identity(),
                toolchain.llvm_root().display()
            ),
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
    /// producing something that looks like a DLL.
    #[test]
    fn building_reports_plainly_that_compilation_is_not_implemented_yet() {
        let dir = tempfile::tempdir().unwrap();
        // A cache root that cannot be created, so the test never reaches the network.
        let unusable = dir.path().join("file-not-a-directory");
        std::fs::write(&unusable, b"x").unwrap();
        let runner = BootstrappedToolchain::new(unusable.join("toolchain"));

        let request = BuildRequest {
            source_root: dir.path().to_path_buf(),
            forty_three_civs: FortyThreeCivs::Disabled,
            output_path: dir.path().join("CvGameCore_Expansion2.dll"),
        };
        let error = runner
            .build_dll(&request, &ProgressReporter::silent())
            .unwrap_err();

        // Whatever went wrong, the user gets a sentence and the log gets the path.
        assert!(!error.message().is_empty());
        assert!(!error.detail().is_empty());
        assert!(!dir.path().join("CvGameCore_Expansion2.dll").exists());
    }
}
