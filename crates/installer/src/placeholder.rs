//! Stand-ins for the Core's three boundaries, for the shell tests and screen previews.
//!
//! The shipped binary wires [`crate::wiring`] instead. These exist so the `egui_kittest`
//! suite can drive a whole install offline:
//!
//! * [`DirectorySourceProvider`] handles the Local Repo case for real — a folder is a folder
//!   — and refuses the Upstream Cache, which would need the network.
//! * [`PlaceholderToolchainRunner`] writes a marker file instead of compiling, keeping the
//!   1.1 GB Toolchain Bootstrap and the multi-minute compile out of the fast suite.
//! * [`PlaceholderModpackAssembler`] writes marker dumps instead of merging databases,
//!   keeping SQLite work out of the shell suite.

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{
    BoundaryError, BuildRequest, CacheState, Core, InstallationSource, MaterializedSource,
    ModpackAssembler, ModpackDatabaseJob, ProgressReporter, SourceProvider, Stage, ToolchainRunner,
};

/// What [`PlaceholderToolchainRunner`] writes where the Built DLL belongs.
pub const PLACEHOLDER_DLL_CONTENTS: &str =
    "Civ 5 VP Installer placeholder. This is NOT a compiled DLL — ticket 06 replaces it.\n";

/// What [`PlaceholderModpackAssembler`] writes where the database dumps belong.
pub const PLACEHOLDER_DUMP_CONTENTS: &str =
    "Civ 5 VP Installer placeholder. This is NOT a database dump.\n";

/// A Core wired to the placeholder boundaries.
pub fn core(work_dir: PathBuf) -> Core {
    Core::new(
        Box::new(DirectorySourceProvider),
        Box::new(PlaceholderToolchainRunner {
            build_dir: work_dir.join("build"),
        }),
        Box::new(PlaceholderModpackAssembler),
        work_dir,
    )
}

/// Serves a Local Repo exactly as it sits on disk — no git operation runs against it.
pub struct DirectorySourceProvider;

impl SourceProvider for DirectorySourceProvider {
    fn materialize(
        &self,
        source: &InstallationSource,
        progress: &ProgressReporter,
    ) -> Result<MaterializedSource, BoundaryError> {
        match source {
            InstallationSource::LocalRepo { path } => {
                if path.as_os_str().is_empty() {
                    return Err(BoundaryError::new(
                        "Choose the folder holding your Community-Patch-DLL checkout.",
                        "local repo path was empty",
                    ));
                }
                if !path.is_dir() {
                    return Err(BoundaryError::new(
                        format!(
                            "There is no folder at {}. Check the path and try again.",
                            path.display()
                        ),
                        format!("local repo path is not a directory: {}", path.display()),
                    ));
                }
                progress.report(
                    Stage::Fetch,
                    format!("Using the checkout at {}.", path.display()),
                );
                // Content-derived, like the real Local Repo provider — so the shell tests
                // exercise the same skip-and-rebuild behaviour the shipped installer has.
                let source_identity =
                    civ5vp_core::dll_source_identity(path).map_err(|unreadable| {
                        BoundaryError::new(
                            format!(
                                "A file in your repository could not be read: {}.",
                                unreadable.display()
                            ),
                            format!("unreadable while fingerprinting: {}", unreadable.display()),
                        )
                    })?;
                Ok(MaterializedSource {
                    root: path.clone(),
                    source_identity,
                })
            }
            InstallationSource::UpstreamCache { .. } => Err(BoundaryError::new(
                "Downloading versions from GitHub is not available in this build yet. \
                 Point the installer at a local checkout instead.",
                "upstream cache provider arrives with ticket 04",
            )),
        }
    }

    fn available_versions(
        &self,
        _progress: &ProgressReporter,
    ) -> Result<civ5vp_core::VersionCatalog, BoundaryError> {
        Ok(fixture_version_catalog())
    }

    fn unofficial_versions(
        &self,
        newest_release: &str,
        _progress: &ProgressReporter,
    ) -> Result<Vec<civ5vp_core::UnofficialVersion>, BoundaryError> {
        Ok(fixture_unofficial_versions(newest_release))
    }

    fn materialize_luajit(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError> {
        progress.report(Stage::Fetch, "Using a placeholder LuaJIT source tree.");
        placeholder_luajit_source()
    }
}

/// An empty stand-in for the pinned LuaJIT checkout: the two directories the build looks for
/// and nothing inside them. The fast suite must not clone LuaJIT, and the placeholder toolchain
/// runner never reads a byte of it.
fn placeholder_luajit_source() -> Result<PathBuf, BoundaryError> {
    let root = std::env::temp_dir().join("civ5vp-placeholder-luajit");
    for directory in ["src", "dynasm"] {
        fs::create_dir_all(root.join(directory)).map_err(|err| {
            BoundaryError::new(
                "Could not prepare the placeholder LuaJIT source folder.",
                format!("placeholder provider: {err}"),
            )
        })?;
    }
    Ok(root)
}

/// The unofficial list every offline surface shares: two changes after the
/// newest Release, the second with a summary far too long for any dropdown — the shape the
/// shell has to cope with.
pub fn fixture_unofficial_versions(newest_release: &str) -> Vec<civ5vp_core::UnofficialVersion> {
    let base = newest_release.trim_start_matches("Release-").to_owned();
    vec![
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
    ]
}

/// The catalog every offline surface shares — the fake provider, the screen previews — so
/// the shell tests and baselines can draw the picker without a socket.
pub fn fixture_version_catalog() -> civ5vp_core::VersionCatalog {
    civ5vp_core::VersionCatalog::from_remote_refs([
        ("refs/tags/Release-5.2", "b".repeat(40)),
        ("refs/tags/Release-5.1", "a".repeat(40)),
        ("refs/heads/master", "c".repeat(40)),
    ])
}

/// Writes a marker where the Built DLL belongs, so the rest of the pipeline can be exercised
/// without a 580 MB toolchain download and a multi-minute compile.
pub struct PlaceholderToolchainRunner {
    /// Stands in for the Toolchain Cache: once a build has run, the first-run note stops —
    /// the same life cycle the real runner has, so the shell tests can watch it disappear.
    build_dir: PathBuf,
}

impl ToolchainRunner for PlaceholderToolchainRunner {
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        progress.report(
            Stage::Build,
            "Writing a placeholder DLL — no compiler runs in this build.",
        );
        fs::write(&request.output_path, PLACEHOLDER_DLL_CONTENTS).map_err(|err| {
            BoundaryError::new(
                format!(
                    "Could not write the DLL to {}. Check that the folder is writable.",
                    request.output_path.display()
                ),
                format!("placeholder runner write failed: {err}"),
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
        fs::write(&request.output_path, "fake luajit engine").map_err(|err| {
            BoundaryError::new(
                "The LuaJIT engine could not be built.",
                format!("fake runner: {err}"),
            )
        })
    }

    fn toolchain_identity(&self) -> String {
        "placeholder-toolchain-0".to_owned()
    }

    fn first_run_expectation(&self) -> Option<String> {
        if self
            .build_dir
            .join(civ5vp_core::BUILT_DLL_FILE_NAME)
            .is_file()
        {
            return None;
        }
        Some(
            "First install downloads about 1.1 GB of build tools — one time — and \
             typically takes 10–25 minutes. Later installs take seconds to minutes."
                .to_owned(),
        )
    }
}

/// Believes any readable cache file is pristine unless it says "modded", and writes marker
/// dumps — the same shape the Core-seam fixture uses, so a shell test can stage a Modpack
/// without a database engine in the loop.
pub struct PlaceholderModpackAssembler;

impl ModpackAssembler for PlaceholderModpackAssembler {
    fn cache_state(&self, gameplay_db: &Path) -> Result<CacheState, BoundaryError> {
        let text = fs::read_to_string(gameplay_db).map_err(|err| {
            BoundaryError::new(
                "The game's database cache could not be read.",
                format!("placeholder assembler: {err}"),
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
        progress.report(Stage::Build, "Placeholder database merge.");
        let write = |path: &Path| {
            fs::write(path, PLACEHOLDER_DUMP_CONTENTS).map_err(|err| {
                BoundaryError::new(
                    "The Modpack databases could not be written.",
                    format!("placeholder assembler: {err}"),
                )
            })
        };
        write(&job.gameplay_dump)?;
        write(&job.text_dump)
    }
}
