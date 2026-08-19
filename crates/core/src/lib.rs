//! The headless Core of the Civ 5 VP Installer.
//!
//! This crate is the single primary seam described in `docs/spec.md`: it accepts an
//! [`InstallConfiguration`] plus resolved [`GameFolders`], produces a [`Plan`], executes it,
//! and reports progress and results. It knows nothing about egui - that is enforced by the
//! crate boundary rather than by convention.
//!
//! Exactly three boundaries are injected into [`Core`]: the [`SourceProvider`] (where mod
//! files and DLL sources come from), the [`ToolchainRunner`] (which compiles the Built DLL),
//! and the [`ModpackAssembler`] (which merges and dumps the Modpack's databases). Everything
//! else - planning, exclusions, Sync - is concrete behind this API.
//!
//! Vocabulary is `CONTEXT.md`'s, exactly.

// No panicking paths in code reachable from the UI. These are crate-level, so the
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
mod detect;
mod error;
mod fingerprint;
mod install;
mod modinfo;
mod modpack;
mod plan;
mod progress;
mod replaced;
mod settings;
mod tree;
mod versions;

pub use boundaries::{
    BoundaryError, BuildRequest, CacheState, LuaJitBuildRequest, MaterializedSource,
    ModpackAssembler, ModpackDatabaseJob, SourceProvider, ToolchainRunner,
};
pub use claimed::{ClaimedFile, ClaimedFolder, DeploymentTarget, GameFolders};
pub use configuration::{
    BuildConfiguration, Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallMode,
    InstallationSource, LuaJitEngine, Version,
};
pub use detect::{
    DetectedGame, Detection, DocumentsFolder, FolderKind, FolderRejected, GameInstallation,
    RejectionReason, SearchLocations, detect_game, game_folders, resolve_game_folders,
    validate_documents_folder, validate_game_installation,
};
pub use error::{GameFolderProblem, InstallError, SourceItem};
pub use fingerprint::dll_source_identity;
pub use install::{Core, InstallOutcome, UninstallOutcome};
pub use modpack::available_extra_mods;
pub use plan::Plan;
pub use progress::{ProgressEvent, ProgressReporter, Stage};
pub use replaced::{BackupStore, EngineOutcome, ReplacedFile, Restored};
pub use settings::{AppDataStore, Settings, SettingsError, Startup, start_up};
pub use versions::{UnofficialVersion, VersionCatalog};

/// The file name of the Built DLL, in the game's MODS Folder and in the build directory.
pub const BUILT_DLL_FILE_NAME: &str = "CvGameCore_Expansion2.dll";
