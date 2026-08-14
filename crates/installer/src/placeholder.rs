//! Stand-ins for the Core's two boundaries, for the shell tests and screen previews.
//!
//! The shipped binary wires [`crate::wiring`] instead. These exist so the `egui_kittest`
//! suite can drive a whole install offline (rule 13):
//!
//! * [`DirectorySourceProvider`] handles the Local Repo case for real — a folder is a folder
//!   — and refuses the Upstream Cache, which would need the network.
//! * [`PlaceholderToolchainRunner`] writes a marker file instead of compiling, keeping the
//!   2.4 GB Toolchain Bootstrap and the multi-minute compile out of the fast suite.

use std::fs;
use std::path::PathBuf;

use civ5vp_core::{
    BoundaryError, BuildRequest, Core, InstallationSource, MaterializedSource, ProgressReporter,
    SourceProvider, Stage, ToolchainRunner,
};

/// What [`PlaceholderToolchainRunner`] writes where the Built DLL belongs.
pub const PLACEHOLDER_DLL_CONTENTS: &str =
    "Civ 5 VP Installer placeholder. This is NOT a compiled DLL — ticket 06 replaces it.\n";

/// A Core wired to the placeholder boundaries.
pub fn core(work_dir: PathBuf) -> Core {
    Core::new(
        Box::new(DirectorySourceProvider),
        Box::new(PlaceholderToolchainRunner),
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
}

/// The catalog every offline surface shares — the fake provider, the screen previews — so
/// the shell tests and baselines can draw the picker without a socket (rule 13).
pub fn fixture_version_catalog() -> civ5vp_core::VersionCatalog {
    civ5vp_core::VersionCatalog::from_remote_refs([
        ("refs/tags/Release-5.2", "b".repeat(40)),
        ("refs/tags/Release-5.1", "a".repeat(40)),
        ("refs/heads/master", "c".repeat(40)),
    ])
}

/// Writes a marker where the Built DLL belongs, so the rest of the pipeline can be exercised
/// without a 580 MB toolchain download and a multi-minute compile.
pub struct PlaceholderToolchainRunner;

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

    fn toolchain_identity(&self) -> String {
        "placeholder-toolchain-0".to_owned()
    }
}
