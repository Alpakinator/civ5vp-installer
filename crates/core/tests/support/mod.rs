//! Shared scaffolding for the Core-seam tests.
//!
//! House style (CODING_STANDARDS.md rule 12): a test gets a fixture repository and temporary
//! MODS/DLC/Text directories, runs an Install Configuration through the public Core API, and
//! asserts on the resulting file tree. Nothing here reaches into the Core.
//!
//! The two injected boundaries are faked (rule 13): [`FixtureSourceProvider`] hands back a
//! committed fixture tree, [`MarkerToolchainRunner`] writes a recognisable marker instead of
//! compiling. So the fast suite never clones, downloads, or compiles anything.

// Each integration test file compiles its own copy of this module, and none of them uses all
// of it — the failure providers belong to `deployment.rs`, the matrix constants to `matrix.rs`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{
    BoundaryError, BuildRequest, GameFolders, InstallationSource, MaterializedSource,
    ProgressReporter, SourceProvider, Stage, ToolchainRunner,
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
}

/// What [`MarkerToolchainRunner`] writes instead of a real DLL.
pub const DLL_MARKER: &str = "marker artifact standing in for the Built DLL";

/// Writes a marker file where the Built DLL would go (rule 13).
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
}

impl CountingToolchainRunner {
    pub fn new(identity: &str) -> (Self, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        (
            Self {
                builds: std::sync::Arc::clone(&builds),
                identity: identity.to_owned(),
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
        MarkerToolchainRunner.build_dll(request, progress)
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
