//! The three — and only three — boundaries injected into the Core.
//!
//! Everything else the installer does is concrete behind [`crate::Core`]. Adding a trait
//! here is an architectural change, not a refactor.

use std::path::PathBuf;

use crate::configuration::{BuildConfiguration, FortyThreeCivs, InstallationSource};
use crate::progress::ProgressReporter;

/// A failure reported by one of the injected boundaries.
///
/// Two strings: `message` is shown to the user, `detail` is the raw git/compiler/IO text
/// and goes to the log.
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

/// What [`SourceProvider::materialize`] hands back: where the tree is, and what its DLL
/// build inputs are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedSource {
    /// Root of the materialized tree. The Core only ever reads from it.
    pub root: PathBuf,
    /// A stable identity of the DLL build inputs in this tree — equal identities mean the
    /// build would read identical bytes.
    ///
    /// For a checked-out Version this derives from the git tree; for a Local Repo, from the
    /// working files under the DLL's input roots (see [`crate::dll_source_identity`]). It is
    /// the source half of the Build Fingerprint.
    pub source_identity: String,
}

/// Boundary one: where the mod files and DLL sources come from.
///
/// Implemented by the Upstream Cache and by the Local Repo path.
/// `Send + Sync` because the shell runs a Deployment on a worker thread.
pub trait SourceProvider: Send + Sync {
    /// Make `source` available on disk and describe what was materialized.
    ///
    /// The Core only ever reads from the returned tree. For a Local Repo that means the
    /// developer's working tree is handed back untouched — no git operation runs against it.
    fn materialize(
        &self,
        source: &InstallationSource,
        progress: &ProgressReporter,
    ) -> Result<MaterializedSource, BoundaryError>;

    /// The Versions the Upstream Cache can offer right now — what the picker lists.
    ///
    /// One remote round trip in production; a fixture in the fast suite. Nothing is fetched
    /// beyond ref names, so this is safe while the user is still deciding.
    fn available_versions(
        &self,
        progress: &ProgressReporter,
    ) -> Result<crate::VersionCatalog, BoundaryError>;

    /// Every commit after `newest_release` (a `Release-*` tag name), oldest first — what
    /// the picker lists as unofficial versions. One small HTTP round trip for
    /// the Upstream Cache; a Local Repo has no notion of this and returns an error saying
    /// so.
    fn unofficial_versions(
        &self,
        newest_release: &str,
        progress: &ProgressReporter,
    ) -> Result<Vec<crate::UnofficialVersion>, BoundaryError>;

    /// The pinned LuaJIT source tree, fetched if it is not cached yet.
    ///
    /// Only called when the configuration opts into the LuaJIT engine, so a player on the
    /// stock engine never fetches it. Returns the directory holding `src/` and `dynasm/`.
    fn materialize_luajit(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError>;
}

/// What the Core asks the toolchain runner to compile.
#[derive(Debug, Clone)]
pub struct BuildRequest {
    /// Root of the materialized Installation Source.
    pub source_root: PathBuf,
    /// Whether to compile with the 43-civ setting.
    pub forty_three_civs: FortyThreeCivs,
    /// Release or Debug — always Release until Dev mode offers the choice.
    pub build_configuration: BuildConfiguration,
    /// Compiled into the DLL as its version string — see
    /// [`InstallationSource::version_label`].
    pub version_label: String,
    /// Exactly where the Built DLL must be written.
    ///
    /// Always inside the Core's own build directory, never in a game folder — the game is
    /// not touched until the build has fully succeeded.
    pub output_path: PathBuf,
}

/// What the Core asks the toolchain runner to compile for the Replaced File.
///
/// A second request type rather than a flag on [`BuildRequest`], because the two builds share
/// only their compiler: different sources, different output, different reasons to fail.
#[derive(Debug, Clone)]
pub struct LuaJitBuildRequest {
    /// Root of the materialized LuaJIT source — the directory holding `src/` and `dynasm/`.
    pub source_root: PathBuf,
    /// The Game Installation root.
    ///
    /// Not a place anything is written: the runner reads the game's own binaries from it to
    /// check that the engine it just built exports everything they import. A DLL missing one
    /// of those symbols is the single failure that would leave a player unable to start the
    /// game, and it is checkable before the game is touched.
    pub game_root: PathBuf,
    /// Exactly where the built engine must be written.
    ///
    /// Always inside the Core's own build directory, never a game folder — the game is not
    /// touched until every part of the Deployment that can fail has succeeded.
    pub output_path: PathBuf,
}

/// Boundary two: compiling the Built DLL.
///
/// The real implementation drives the bootstrapped clang from the Toolchain Cache.
pub trait ToolchainRunner: Send + Sync {
    /// Compile the DLL and write it to [`BuildRequest::output_path`].
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError>;

    /// Compile LuaJIT and write the engine to [`LuaJitBuildRequest::output_path`].
    ///
    /// Called only when the configuration opts into the LuaJIT engine, so a player on the
    /// stock engine never compiles it. Like [`Self::build_dll`], this must never write into a
    /// game folder — Sync decides when the game is touched.
    fn build_luajit(
        &self,
        request: &LuaJitBuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError>;

    /// A stable identifier for this toolchain, e.g. `clang-18.1.8`. Folded into the Build
    /// Fingerprint.
    fn toolchain_identity(&self) -> String;

    /// A sentence to show *before* the first Deployment, while getting the toolchain still
    /// costs a download — `None` once it is set up (the multi-GB download must not be a
    /// surprise, and the warning must stop being said the moment it stops being true). The
    /// default is `None`: a runner with nothing to warn about says nothing.
    fn first_run_expectation(&self) -> Option<String> {
        None
    }
}

/// Whether a game cache database can serve as the Modpack's base.
///
/// The Modpack build starts from the game's own merged vanilla database
/// (`cache/Civ5DebugDatabase.db` after an unmodded launch). A launch with mods activated
/// rewrites that file with the mods applied, and a Modpack built on top of it would bake
/// everything in twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    /// The vanilla base+DLC merge — usable as the Modpack base.
    Pristine,
    /// A modded session wrote this file; the user must launch the game unmodded once.
    Modded,
}

/// What the Core asks the modpack assembler to merge and dump.
///
/// The Core stages every file of the Modpack itself; the assembler only does the part that
/// needs a database engine: apply the mods' updates to copies of the two base databases and
/// write the two Override dumps the game will load instead of its own XML.
#[derive(Debug, Clone)]
pub struct ModpackDatabaseJob {
    /// The pristine gameplay database snapshot (never written; the assembler copies it).
    pub gameplay_base: PathBuf,
    /// The pristine localization database snapshot (never written; the assembler copies it).
    pub text_base: PathBuf,
    /// The mods' database update files, in activation order — `.sql` executed as SQL,
    /// `.xml` applied with the game's GameData semantics. `Language_*` tables route to the
    /// localization database, everything else to the gameplay database, exactly as the game
    /// routes them.
    pub updates: Vec<PathBuf>,
    /// Where the full gameplay dump is written (`Override/CIV5Units.xml` in the stage).
    pub gameplay_dump: PathBuf,
    /// Where the localization dump is written (`Override/CIV5Units_Mongol.xml`).
    pub text_dump: PathBuf,
    /// Scratch space owned by the assembler for the working database copies.
    pub scratch_dir: PathBuf,
}

/// Boundary three: the Modpack's database merge.
///
/// A separate boundary for the same reason the toolchain is one: the work needs machinery —
/// a SQLite engine — that must stay out of the dependency-free Core, and tests need to
/// stand in a fake for it.
pub trait ModpackAssembler: Send + Sync {
    /// Whether `gameplay_db` is a usable Modpack base — see [`CacheState`].
    fn cache_state(&self, gameplay_db: &std::path::Path) -> Result<CacheState, BoundaryError>;

    /// Apply the updates to copies of the base databases and write both dumps.
    fn merge_and_dump(
        &self,
        job: &ModpackDatabaseJob,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError>;
}
