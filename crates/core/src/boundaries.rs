//! The two — and only two — boundaries injected into the Core (rule 2).
//!
//! Everything else the installer does is concrete behind [`crate::Core`]. Adding a third
//! trait here is an architectural change, not a refactor.

use std::path::PathBuf;

use crate::configuration::{FortyThreeCivs, InstallationSource};
use crate::progress::ProgressReporter;

/// A failure reported by one of the injected boundaries.
///
/// Two strings, because rule 10 wants both: `message` is shown to the user, `detail` is the
/// raw git/compiler/IO text and goes to the log.
#[derive(Debug, Clone)]
pub struct BoundaryError {
    message: String,
    detail: String,
}

impl BoundaryError {
    pub fn new(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            detail: detail.into(),
        }
    }

    /// A sentence a non-programmer can act on.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Everything else.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BoundaryError {}

/// Boundary one: where the mod files and DLL sources come from.
///
/// Implemented by the Upstream Cache (ticket 04) and by the Local Repo path (ticket 08).
/// `Send + Sync` because the shell runs a Deployment on a worker thread.
pub trait SourceProvider: Send + Sync {
    /// Make `source` available on disk and return the root of the resulting tree.
    ///
    /// The Core only ever reads from the returned path. For a Local Repo that means the
    /// developer's working tree is handed back untouched — no git operation runs against it.
    fn materialize(
        &self,
        source: &InstallationSource,
        progress: &ProgressReporter,
    ) -> Result<PathBuf, BoundaryError>;
}

/// What the Core asks the toolchain runner to compile.
#[derive(Debug, Clone)]
pub struct BuildRequest {
    /// Root of the materialized Installation Source.
    pub source_root: PathBuf,
    /// Whether to compile with the 43-civ setting.
    pub forty_three_civs: FortyThreeCivs,
    /// Exactly where the Built DLL must be written.
    ///
    /// Always inside the Core's own build directory, never in a game folder — rule 7 means
    /// the game is not touched until the build has fully succeeded.
    pub output_path: PathBuf,
}

/// Boundary two: compiling the Built DLL.
///
/// Implemented for real by ticket 06, driving the bootstrapped clang from the Toolchain Cache.
pub trait ToolchainRunner: Send + Sync {
    /// Compile the DLL and write it to [`BuildRequest::output_path`].
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError>;

    /// A stable identifier for this toolchain, e.g. `clang-18.1.8`.
    ///
    /// Ticket 07 folds this into the Build Fingerprint; today it only appears in the log.
    fn toolchain_identity(&self) -> String;
}
