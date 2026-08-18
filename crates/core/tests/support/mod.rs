//! Shared scaffolding for the Core-seam tests.
//!
//! House style: a test gets a fixture repository and temporary
//! MODS/DLC/Text directories, runs an Install Configuration through the public Core API, and
//! asserts on the resulting file tree. Nothing here reaches into the Core.
//!
//! The two injected boundaries are faked: [`FixtureSourceProvider`] hands back a
//! committed fixture tree, [`MarkerToolchainRunner`] writes a recognisable marker instead of
//! compiling. So the fast suite never clones, downloads, or compiles anything.

// Each integration test file compiles its own copy of this module, and none of them uses all
// of it — the failure providers belong to `deployment.rs`, the matrix constants to `matrix.rs`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{
    BoundaryError, BuildRequest, CacheState, GameFolders, InstallationSource, MaterializedSource,
    ModpackAssembler, ModpackDatabaseJob, ProgressReporter, SourceProvider, Stage, ToolchainRunner,
};

/// The miniature Community-Patch-DLL layout committed under `tests/fixtures/`.
pub fn miniature_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/miniature-repo")
}

/// Serves a fixture tree as-is, the way the Local Repo provider will.
pub struct FixtureSourceProvider {
    root: PathBuf,
}

impl FixtureSourceProvider {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl SourceProvider for FixtureSourceProvider {
    fn materialize(
        &self,
        _source: &InstallationSource,
        progress: &ProgressReporter,
    ) -> Result<MaterializedSource, BoundaryError> {
        progress.report(Stage::Fetch, "Using the fixture repository.");
        // Content-derived, the way the Local Repo provider does it — so a test that edits a
        // fixture source file really changes the identity the Core sees.
        let source_identity = civ5vp_core::dll_source_identity(&self.root).map_err(|path| {
            BoundaryError::new(
                "A fixture file could not be read.",
                format!("unreadable: {}", path.display()),
            )
        })?;
        Ok(MaterializedSource {
            root: self.root.clone(),
            source_identity,
        })
    }

    fn available_versions(
        &self,
        _progress: &ProgressReporter,
    ) -> Result<civ5vp_core::VersionCatalog, BoundaryError> {
        // The catalog a fixture upstream would advertise: two Releases and a master.
        Ok(civ5vp_core::VersionCatalog::from_remote_refs([
            ("refs/tags/Release-2.0", "b".repeat(40)),
            ("refs/tags/Release-1.0", "a".repeat(40)),
            ("refs/heads/master", "c".repeat(40)),
        ]))
    }

    fn unofficial_versions(
        &self,
        newest_release: &str,
        _progress: &ProgressReporter,
    ) -> Result<Vec<civ5vp_core::UnofficialVersion>, BoundaryError> {
        // Two changes after the newest Release, the second with a summary far too long for
        // any dropdown — the shape the shell has to cope with.
        let base = newest_release.trim_start_matches("Release-").to_owned();
        Ok(vec![
            civ5vp_core::UnofficialVersion {
                label: format!("{base}.01"),
                summary: "Fix a promotion".to_owned(),
                commit: "c".repeat(40),
            },
            civ5vp_core::UnofficialVersion {
                label: format!("{base}.02"),
                summary: "A very long commit message that certainly does not fit into the \
                          width of any dropdown a version picker could reasonably draw"
                    .to_owned(),
                commit: "d".repeat(40),
            },
        ])
    }

    fn materialize_luajit(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError> {
        progress.report(Stage::Fetch, "Using a fixture LuaJIT source tree.");
        // Empty, and not inside the committed fixture: nothing in the fast suite compiles
        // LuaJIT, so only the two directories the build looks for need to exist.
        let root = std::env::temp_dir().join("civ5vp-fixture-luajit");
        for directory in ["src", "dynasm"] {
            fs::create_dir_all(root.join(directory)).map_err(|err| {
                BoundaryError::new(
                    "The fixture LuaJIT source folder could not be prepared.",
                    format!("fixture provider: {err}"),
                )
            })?;
        }
        Ok(root)
    }
}

/// A source provider that always fails, for the abort-before-touch case.
pub struct FailingSourceProvider;

impl SourceProvider for FailingSourceProvider {
    fn materialize(
        &self,
        _source: &InstallationSource,
        _progress: &ProgressReporter,
    ) -> Result<MaterializedSource, BoundaryError> {
        Err(BoundaryError::new(
            "Could not download the mod files. Check your internet connection and try again.",
            "fake provider: simulated network failure",
        ))
    }

    fn available_versions(
        &self,
        _progress: &ProgressReporter,
    ) -> Result<civ5vp_core::VersionCatalog, BoundaryError> {
        Err(BoundaryError::new(
            "Could not look up the available versions. Check your internet connection and \
             try again.",
            "fake provider: simulated network failure",
        ))
    }

    fn unofficial_versions(
        &self,
        _newest_release: &str,
        _progress: &ProgressReporter,
    ) -> Result<Vec<civ5vp_core::UnofficialVersion>, BoundaryError> {
        Err(BoundaryError::new(
            "Could not look up the changes since the newest release. Check your internet \
             connection and try again.",
            "fake provider: simulated network failure",
        ))
    }

    fn materialize_luajit(&self, _progress: &ProgressReporter) -> Result<PathBuf, BoundaryError> {
        Err(BoundaryError::new(
            "Could not download the LuaJIT source. Check your internet connection and try \
             again.",
            "fake provider: simulated network failure",
        ))
    }
}

/// What [`FixtureModpackAssembler`] writes instead of the real database dumps.
pub const GAMEPLAY_DUMP_MARKER: &str = "marker standing in for the merged gameplay dump";
pub const TEXT_DUMP_MARKER: &str = "marker standing in for the merged text dump";

/// The third boundary, faked: reads a marker instead of a database, writes
/// markers instead of dumps, and remembers every job so a test can assert what crossed
/// the seam.
pub struct FixtureModpackAssembler {
    jobs: std::sync::Arc<std::sync::Mutex<Vec<ModpackDatabaseJob>>>,
}

impl FixtureModpackAssembler {
    pub fn new() -> (
        Self,
        std::sync::Arc<std::sync::Mutex<Vec<ModpackDatabaseJob>>>,
    ) {
        let jobs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (Self { jobs: jobs.clone() }, jobs)
    }

    /// The common case: the recorded jobs are not asserted on.
    pub fn ignored() -> Self {
        Self::new().0
    }
}

impl ModpackAssembler for FixtureModpackAssembler {
    fn cache_state(&self, gameplay_db: &Path) -> Result<CacheState, BoundaryError> {
        // A cache fixture says what it is: a file containing "modded" is one a modded
        // session wrote.
        let text = fs::read_to_string(gameplay_db).map_err(|err| {
            BoundaryError::new(
                "The game's database cache could not be read.",
                format!("fake assembler: {err}"),
            )
        })?;
        if text.contains("modded") {
            Ok(CacheState::Modded)
        } else {
            Ok(CacheState::Pristine)
        }
    }

    fn merge_and_dump(
        &self,
        job: &ModpackDatabaseJob,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        progress.report(Stage::Build, "Faking the database merge.");
        let write = |path: &Path, marker: &str| {
            fs::write(path, marker).map_err(|err| {
                BoundaryError::new(
                    "The Modpack databases could not be written.",
                    format!("fake assembler could not write the marker: {err}"),
                )
            })
        };
        write(&job.gameplay_dump, GAMEPLAY_DUMP_MARKER)?;
        write(&job.text_dump, TEXT_DUMP_MARKER)?;
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.push(job.clone());
        }
        Ok(())
    }
}

/// What [`MarkerToolchainRunner`] writes instead of a real DLL.
pub const DLL_MARKER: &str = "marker artifact standing in for the Built DLL";

/// Writes a marker file where the Built DLL would go.
pub struct MarkerToolchainRunner;

impl ToolchainRunner for MarkerToolchainRunner {
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        progress.report(Stage::Build, "Faking a DLL build.");
        fs::write(&request.output_path, DLL_MARKER).map_err(|err| {
            BoundaryError::new(
                "The DLL build failed.",
                format!("fake runner could not write the marker: {err}"),
            )
        })
    }

    fn build_luajit(
        &self,
        request: &civ5vp_core::LuaJitBuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        progress.report(Stage::Build, "Faking a LuaJIT build.");
        if let Some(parent) = request.output_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&request.output_path, LUAJIT_MARKER).map_err(|err| {
            BoundaryError::new(
                "The LuaJIT engine could not be built.",
                format!("fake runner could not write the marker: {err}"),
            )
        })
    }

    fn toolchain_identity(&self) -> String {
        "fake-toolchain-0".to_owned()
    }
}

/// What [`MarkerToolchainRunner`] writes instead of a real Lua engine.
pub const LUAJIT_MARKER: &str = "marker artifact standing in for the LuaJIT engine";

/// Builds the DLL happily and fails only on the engine — the shape that proves Sync never
/// starts when the *second* thing that can fail does.
pub struct FailingLuaJitToolchainRunner;

impl ToolchainRunner for FailingLuaJitToolchainRunner {
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        MarkerToolchainRunner.build_dll(request, progress)
    }

    fn build_luajit(
        &self,
        _request: &civ5vp_core::LuaJitBuildRequest,
        _progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        Err(BoundaryError::new(
            "The LuaJIT engine could not be built, so your game was not changed.",
            "fake runner: simulated LuaJIT compile failure",
        ))
    }

    fn toolchain_identity(&self) -> String {
        "fake-toolchain-0".to_owned()
    }
}

/// A [`MarkerToolchainRunner`] that also counts how often it is asked to build — how the
/// fingerprint tests observe, from outside the Core, whether the build was skipped.
///
/// The counter is shared: the Core takes the runner by `Box`, so the test keeps a clone of
/// the `Arc` and reads it after the fact. `identity` is configurable because a different
/// Toolchain version must invalidate the fingerprint.
pub struct CountingToolchainRunner {
    pub builds: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub identity: String,
    /// Every Build Configuration the boundary was asked for, in order.
    pub configurations: std::sync::Arc<std::sync::Mutex<Vec<civ5vp_core::BuildConfiguration>>>,
}

impl CountingToolchainRunner {
    pub fn new(identity: &str) -> (Self, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        (
            Self {
                builds: std::sync::Arc::clone(&builds),
                identity: identity.to_owned(),
                configurations: std::sync::Arc::default(),
            },
            builds,
        )
    }
}

impl ToolchainRunner for CountingToolchainRunner {
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut seen) = self.configurations.lock() {
            seen.push(request.build_configuration);
        }
        MarkerToolchainRunner.build_dll(request, progress)
    }

    fn build_luajit(
        &self,
        request: &civ5vp_core::LuaJitBuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        MarkerToolchainRunner.build_luajit(request, progress)
    }

    fn toolchain_identity(&self) -> String {
        self.identity.clone()
    }
}

/// A toolchain runner that always fails, for the abort-before-touch case.
pub struct FailingToolchainRunner;

impl ToolchainRunner for FailingToolchainRunner {
    fn build_dll(
        &self,
        _request: &BuildRequest,
        _progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        Err(BoundaryError::new(
            "The DLL could not be built.",
            "fake runner: simulated compile failure",
        ))
    }

    fn build_luajit(
        &self,
        _request: &civ5vp_core::LuaJitBuildRequest,
        _progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        Err(BoundaryError::new(
            "The LuaJIT engine could not be built.",
            "fake runner: simulated compile failure",
        ))
    }

    fn toolchain_identity(&self) -> String {
        "fake-toolchain-0".to_owned()
    }
}

/// Temporary MODS / DLC / Text directories laid out the way a real install has them, plus a
/// work directory for the Core.
pub struct GameFixture {
    temp: tempfile::TempDir,
}

impl GameFixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let fixture = Self { temp };
        for folder in [
            fixture.game_root().join("MODS"),
            fixture.game_root().join("DLC"),
            fixture.game_root().join("Text"),
            // The Documents side of a real install has these beside MODS and Text. `cache` is
            // cleared after every Deployment; `ModUserData` holds the player's own mod saves
            // and is never touched.
            fixture.game_root().join("cache"),
            fixture.game_root().join("ModUserData"),
            fixture.work_dir(),
        ] {
            fs::create_dir_all(folder).unwrap();
        }
        fixture
    }

    /// Everything the installer may see. The three game folders live under here so a single
    /// listing can prove that nothing outside the Claimed Folders moved.
    pub fn game_root(&self) -> PathBuf {
        self.temp.path().join("game")
    }

    /// Scratch space the Core owns — stands in for the App Data Store.
    pub fn work_dir(&self) -> PathBuf {
        self.temp.path().join("app-data")
    }

    pub fn folders(&self) -> GameFolders {
        GameFolders {
            mods: self.game_root().join("MODS"),
            dlc: self.game_root().join("DLC"),
            text: self.game_root().join("Text"),
            // The fixture collapses the two sides of a real install into one root, so the
            // Game Installation is that same directory — which is what puts the Replaced File
            // inside `files()`, where a test can see it.
            game_root: self.game_root(),
        }
    }

    /// Every file under the game root, as `/`-separated paths relative to it, sorted.
    pub fn files(&self) -> Vec<String> {
        let mut found = Vec::new();
        collect_files(&self.game_root(), &self.game_root(), &mut found);
        found.sort();
        found
    }

    /// Read a file by its path relative to the game root.
    pub fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.game_root().join(relative)).unwrap()
    }

    /// Put a file into the game root, creating parents. Used to plant decoys and stale files.
    pub fn plant(&self, relative: &str, contents: &str) {
        let path = self.game_root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

fn collect_files(root: &Path, dir: &Path, found: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_files(root, &path, found);
        } else {
            let relative = path.strip_prefix(root).unwrap();
            found.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}
