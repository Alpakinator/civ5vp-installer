//! The headless Core of the Civ 5 VP Installer.
//!
//! This crate is the single primary seam described in `docs/spec.md`: it accepts an
//! [`InstallConfiguration`] plus resolved [`GameFolders`], produces a [`Plan`], executes it,
//! and reports progress and results. It knows nothing about egui — that is enforced by the
//! crate boundary rather than by convention (`CODING_STANDARDS.md` rule 1).
//!
//! Exactly two boundaries are injected into [`Core`]: the [`SourceProvider`] (where mod files
//! and DLL sources come from) and the [`ToolchainRunner`] (which compiles the Built DLL).
//! Everything else — planning, exclusions, Sync — is concrete behind this API (rule 2).
//!
//! Vocabulary is `CONTEXT.md`'s, exactly (rule 16).

// Rule 9: no panicking paths in code reachable from the UI. These are crate-level, so the
// integration tests under `tests/` (separate crates) are free to `unwrap` as usual.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

mod boundaries;
mod claimed;
mod configuration;
mod error;
mod install;
mod plan;
mod progress;
mod tree;

pub use boundaries::{BoundaryError, BuildRequest, SourceProvider, ToolchainRunner};
pub use claimed::{ClaimedFile, ClaimedFolder, DeploymentTarget, GameFolders};
pub use configuration::{
    Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallationSource, Version,
};
pub use error::{GameFolderProblem, InstallError, SourceItem};
pub use install::{Core, InstallOutcome, UninstallOutcome};
pub use plan::Plan;
pub use progress::{ProgressEvent, ProgressReporter, Stage};

/// The file name of the Built DLL, in the game's MODS Folder and in the build directory.
pub const BUILT_DLL_FILE_NAME: &str = "CvGameCore_Expansion2.dll";
