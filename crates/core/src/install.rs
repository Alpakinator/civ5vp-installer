//! The Core itself: plan, then execute in the order fetch → build → Sync.

use std::path::{Path, PathBuf};

use crate::BUILT_DLL_FILE_NAME;
use crate::boundaries::{BuildRequest, SourceProvider, ToolchainRunner};
use crate::claimed::{ClaimedFile, ClaimedFolder, GameFolders};
use crate::configuration::InstallConfiguration;
use crate::error::{InstallError, SourceItem};
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
    /// Claimed Files deployed, in a fixed order.
    pub deployed_files: Vec<ClaimedFile>,
    /// Where the Built DLL ended up in the game.
    pub built_dll: PathBuf,
}

/// What an Uninstall removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallOutcome {
    /// Claimed Folders that were present and have been removed, in a fixed order.
    pub removed: Vec<ClaimedFolder>,
    /// Claimed Files that were present and have been removed, in a fixed order.
    pub removed_files: Vec<ClaimedFile>,
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

    /// One Claimed Folder and the folder in *this* Installation Source that fills it.
    ///
    /// Separate from [`crate::plan::FolderDeployment`] because the source folder's name is a
    /// fact about the Version in hand, not about the configuration: a Plan is made before
    /// anything is fetched, and older Versions spell some of these differently.
    fn resolve_sources(
        &self,
        plan: &Plan,
        source_root: &Path,
    ) -> Result<Vec<(usize, PathBuf)>, InstallError> {
        let mut resolved = Vec::new();
        for (index, deployment) in plan.deployments.iter().enumerate() {
            let candidates = deployment.source_candidates();
            let found = candidates
                .iter()
                .map(|name| source_root.join(name))
                .find(|path| path.is_dir());
            let Some(path) = found else {
                return Err(InstallError::MissingInSource {
                    item: SourceItem::Folder,
                    name: deployment.claimed.folder_name().to_owned(),
                    path: source_root.join(deployment.claimed.folder_name()),
                });
            };
            if !tree::holds_anything_selected(&path, &deployment.selection)? {
                return Err(InstallError::MissingInSource {
                    item: SourceItem::Contents,
                    name: deployment.claimed.folder_name().to_owned(),
                    path,
                });
            }
            resolved.push((index, path));
        }
        Ok(resolved)
    }

    fn fetch(&self, plan: &Plan, progress: &ProgressReporter) -> Result<PathBuf, InstallError> {
        progress.report(Stage::Fetch, "Getting the mod files ready.");
        let source_root = self
            .source_provider
            .materialize(&plan.configuration.source, progress)
            .map_err(InstallError::Fetch)?;

        // Check everything the plan needs is actually there before the build burns minutes —
        // and, more importantly, before Sync starts deleting (rule 7).
        self.resolve_sources(plan, &source_root)?;
        for file in &plan.files {
            let path = source_root.join(file.source_path());
            if !path.is_file() {
                return Err(InstallError::MissingInSource {
                    item: SourceItem::File,
                    name: file.source_path().to_owned(),
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
            for path in folder.every_path_in(&plan.folders) {
                if !path.exists() {
                    continue;
                }
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
        removed.dedup();

        // Claimed Files sit among content the installer does not own, so they are removed one
        // by one rather than with the folder around them.
        for file in plan.removed_files() {
            let path = file.path_in(&plan.folders);
            if path.exists() {
                tree::remove_file_if_present(&path)?;
                progress.report(
                    Stage::Sync,
                    format!("Removed {} — not part of this install.", file.file_name()),
                );
            }
        }

        let sources = self.resolve_sources(plan, source_root)?;
        let mut deployed = Vec::new();
        for (index, from) in sources {
            let Some(deployment) = plan.deployments.get(index) else {
                continue;
            };
            let destination = deployment.claimed.path_in(&plan.folders);
            // Replace rather than merge: this is what makes Sync exact — no file from a
            // previous configuration can survive inside a Claimed Folder. Every *other* name
            // this folder has gone by is removed too, so installing a newer Version over an
            // older one cannot leave the game with both.
            for stale in deployment.claimed.every_path_in(&plan.folders) {
                if stale != destination {
                    tree::remove_if_present(&stale)?;
                }
            }
            tree::remove_if_present(&destination)?;
            tree::copy_selected(&from, &destination, &deployment.selection)?;
            progress.report(
                Stage::Sync,
                format!("Installed {}.", deployment.claimed.folder_name()),
            );
            deployed.push(deployment.claimed);
        }
        deployed.sort_unstable();

        for file in &plan.files {
            tree::copy_file(
                &source_root.join(file.source_path()),
                &file.path_in(&plan.folders),
            )?;
            progress.report(Stage::Sync, format!("Installed {}.", file.file_name()));
        }

        // Every Flavor includes the Community Patch, and the Built DLL is the only DLL
        // deployed — it goes at the root of `(1) Community Patch`.
        let dll_destination = ClaimedFolder::CommunityPatch
            .path_in(&plan.folders)
            .join(BUILT_DLL_FILE_NAME);
        tree::copy_file(built_dll, &dll_destination)?;
        progress.report(Stage::Sync, "Installed the DLL.");

        clear_game_cache(&plan.folders, progress)?;

        Ok(InstallOutcome {
            deployed,
            removed,
            deployed_files: plan.files.clone(),
            built_dll: dll_destination,
        })
    }

    /// Remove every Claimed Folder, restoring an unmodded game.
    ///
    /// Uninstall does not need an Installation Source, a Version, or a build — it is Sync's
    /// removal half on its own, applied to the whole Claimed set rather than to the part of it
    /// that a configuration leaves out.
    pub fn uninstall(
        &self,
        folders: &GameFolders,
        progress: &ProgressReporter,
    ) -> Result<UninstallOutcome, InstallError> {
        folders.check()?;
        progress.report(Stage::Sync, "Removing Vox Populi from the game.");

        let mut removed = Vec::new();
        for folder in ClaimedFolder::ALL {
            for path in folder.every_path_in(folders) {
                if !path.exists() {
                    continue;
                }
                tree::remove_if_present(&path)?;
                progress.report(Stage::Sync, format!("Removed {}.", folder.folder_name()));
                removed.push(folder);
            }
        }
        removed.dedup();

        let mut removed_files = Vec::new();
        for file in ClaimedFile::ALL {
            let path = file.path_in(folders);
            if path.exists() {
                tree::remove_file_if_present(&path)?;
                progress.report(Stage::Sync, format!("Removed {}.", file.file_name()));
                removed_files.push(file);
            }
        }

        clear_game_cache(folders, progress)?;
        progress.report(Stage::Sync, "Your game is back to how it was.");

        Ok(UninstallOutcome {
            removed,
            removed_files,
        })
    }
}

/// Empty the game's `cache` folder — the one path outside the Claimed Folders the installer
/// may touch (rule 6), and the fix for the stale-cache corruption the community works around
/// by hand (user story 23). `ModUserData` is its sibling and is deliberately left alone.
fn clear_game_cache(
    folders: &GameFolders,
    progress: &ProgressReporter,
) -> Result<(), InstallError> {
    // `check` has already established that the MODS and Text Folders agree on where the game
    // is, so this is a real location rather than a guess.
    let Some(cache) = folders.cache() else {
        return Ok(());
    };
    tree::clear_directory_contents(&cache)?;
    progress.report(Stage::Sync, "Cleared the game's cache.");
    Ok(())
}
