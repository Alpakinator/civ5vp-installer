//! The Core itself: plan, then execute in the order fetch → build → Sync.

use std::path::{Path, PathBuf};

use crate::BUILT_DLL_FILE_NAME;
use crate::boundaries::{BuildRequest, SourceProvider, ToolchainRunner};
use crate::claimed::{ClaimedFolder, GameFolders};
use crate::configuration::InstallConfiguration;
use crate::error::InstallError;
use crate::plan::Plan;
use crate::progress::{ProgressReporter, Stage};
use crate::tree;

/// What a finished Deployment did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// Claimed Folders created or refreshed, in a fixed order.
    pub deployed: Vec<ClaimedFolder>,
    /// Claimed Folders that were present but do not belong to this configuration.
    pub removed: Vec<ClaimedFolder>,
    /// Where the Built DLL ended up in the game.
    pub built_dll: PathBuf,
}

/// The headless Core.
///
/// Construct it with its two boundaries and a work directory it owns, then [`Core::plan`] an
/// [`InstallConfiguration`] and [`Core::execute`] the result.
pub struct Core {
    source_provider: Box<dyn SourceProvider>,
    toolchain_runner: Box<dyn ToolchainRunner>,
    work_dir: PathBuf,
}

impl Core {
    /// `work_dir` is scratch space the Core owns — the build directory lives inside it.
    /// It belongs in the App Data Store; ticket 03 resolves that location, tests pass a
    /// temporary directory. It is never a game folder: rule 7 requires the DLL to be built
    /// somewhere the game cannot see before Sync runs.
    pub fn new(
        source_provider: Box<dyn SourceProvider>,
        toolchain_runner: Box<dyn ToolchainRunner>,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            source_provider,
            toolchain_runner,
            work_dir,
        }
    }

    /// Work out what this configuration implies. Rejects configurations that cannot be
    /// deployed before anything at all has happened.
    pub fn plan(
        &self,
        configuration: &InstallConfiguration,
        folders: &GameFolders,
    ) -> Result<Plan, InstallError> {
        Plan::build(configuration, folders)
    }

    /// Fetch, build, then Sync — in that order, and the game is not touched until the first
    /// two have fully succeeded (rule 7).
    pub fn execute(
        &self,
        plan: &Plan,
        progress: &ProgressReporter,
    ) -> Result<InstallOutcome, InstallError> {
        let source_root = self.fetch(plan, progress)?;
        let built_dll = self.build(plan, &source_root, progress)?;
        self.sync(plan, &source_root, &built_dll, progress)
    }

    fn fetch(&self, plan: &Plan, progress: &ProgressReporter) -> Result<PathBuf, InstallError> {
        progress.report(Stage::Fetch, "Getting the mod files ready.");
        let source_root = self
            .source_provider
            .materialize(&plan.configuration.source, progress)
            .map_err(InstallError::Fetch)?;

        // Check everything the plan needs is actually there before the build burns minutes.
        for deployment in &plan.deployments {
            let path = source_root.join(&deployment.source_subdir);
            if !path.is_dir() {
                return Err(InstallError::MissingInSource {
                    folder_name: deployment.source_subdir.clone(),
                    path,
                });
            }
        }

        progress.report(Stage::Fetch, "Mod files ready.");
        Ok(source_root)
    }

    /// Build the DLL into the Core's own build directory and return its path there.
    fn build(
        &self,
        plan: &Plan,
        source_root: &Path,
        progress: &ProgressReporter,
    ) -> Result<PathBuf, InstallError> {
        let build_dir = self.work_dir.join("build");
        tree::create_dir_all(&build_dir)?;
        let output_path = build_dir.join(BUILT_DLL_FILE_NAME);

        // Drop any DLL left by an earlier run, so a failed build cannot pass off a stale
        // artifact as a fresh one. Everything else in the build directory stays: ticket 06
        // recompiles incrementally out of it.
        tree::remove_file_if_present(&output_path)?;

        progress.report(
            Stage::Build,
            format!(
                "Building the DLL with {}.",
                self.toolchain_runner.toolchain_identity()
            ),
        );
        let request = BuildRequest {
            source_root: source_root.to_path_buf(),
            forty_three_civs: plan.configuration.forty_three_civs,
            output_path: output_path.clone(),
        };
        self.toolchain_runner
            .build_dll(&request, progress)
            .map_err(InstallError::Build)?;

        if !output_path.is_file() {
            return Err(InstallError::MissingBuiltDll {
                expected: output_path,
            });
        }

        progress.report(Stage::Build, "DLL built.");
        Ok(output_path)
    }

    /// Make the Claimed Folders match the configuration exactly, and touch nothing else.
    fn sync(
        &self,
        plan: &Plan,
        source_root: &Path,
        built_dll: &Path,
        progress: &ProgressReporter,
    ) -> Result<InstallOutcome, InstallError> {
        progress.report(Stage::Sync, "Installing into the game.");

        let mut removed = Vec::new();
        for folder in plan.removed_folders() {
            let path = folder.path_in(&plan.folders);
            if path.exists() {
                tree::remove_if_present(&path)?;
                progress.report(
                    Stage::Sync,
                    format!(
                        "Removed {} — not part of this install.",
                        folder.folder_name()
                    ),
                );
                removed.push(folder);
            }
        }

        let mut deployed = Vec::new();
        for deployment in &plan.deployments {
            let destination = deployment.claimed.path_in(&plan.folders);
            // Replace rather than merge: this is what makes Sync exact — no file from a
            // previous configuration can survive inside a Claimed Folder.
            tree::remove_if_present(&destination)?;
            tree::copy_tree(&source_root.join(&deployment.source_subdir), &destination)?;
            progress.report(
                Stage::Sync,
                format!("Installed {}.", deployment.claimed.folder_name()),
            );
            deployed.push(deployment.claimed);
        }
        deployed.sort_unstable();

        // Every Flavor includes the Community Patch, and the Built DLL is the only DLL
        // deployed — it goes at the root of `(1) Community Patch`.
        let dll_destination = ClaimedFolder::CommunityPatch
            .path_in(&plan.folders)
            .join(BUILT_DLL_FILE_NAME);
        tree::copy_file(built_dll, &dll_destination)?;
        progress.report(Stage::Sync, "Installed the DLL.");

        Ok(InstallOutcome {
            deployed,
            removed,
            built_dll: dll_destination,
        })
    }
}
