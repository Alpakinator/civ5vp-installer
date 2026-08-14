//! The Core itself: plan, then execute in the order fetch → build → Sync.

use std::path::{Path, PathBuf};

use crate::BUILT_DLL_FILE_NAME;
use crate::boundaries::{
    BuildRequest, MaterializedSource, ModpackAssembler, SourceProvider, ToolchainRunner,
};
use crate::claimed::{ClaimedFile, ClaimedFolder, GameFolders};
use crate::configuration::InstallConfiguration;
use crate::error::{InstallError, SourceItem};
use crate::fingerprint::{BuildFingerprint, FINGERPRINT_FILE_NAME, fnv1a64_of_file};
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
    modpack_assembler: Box<dyn ModpackAssembler>,
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
        modpack_assembler: Box<dyn ModpackAssembler>,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            source_provider,
            toolchain_runner,
            modpack_assembler,
            work_dir,
        }
    }

    /// What the Version picker lists, straight from the source-provider boundary.
    pub fn available_versions(
        &self,
        progress: &ProgressReporter,
    ) -> Result<crate::VersionCatalog, InstallError> {
        self.source_provider
            .available_versions(progress)
            .map_err(InstallError::Fetch)
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
    ///
    /// The build is skipped when the Build Fingerprint recorded at the last Deployment still
    /// matches this configuration *and* the deployed DLL still hashes to what was recorded —
    /// both, so neither a changed input nor a tampered DLL can survive (user story 17).
    pub fn execute(
        &self,
        plan: &Plan,
        progress: &ProgressReporter,
    ) -> Result<InstallOutcome, InstallError> {
        let source = self.fetch(plan, progress)?;
        let fingerprint = BuildFingerprint::new(
            &source.source_identity,
            &plan.configuration.source.version_label(),
            plan.configuration.build_configuration,
            plan.configuration.forty_three_civs,
            &self.toolchain_runner.toolchain_identity(),
        );
        let built_dll = match self.reusable_deployed_dll(plan, &fingerprint, progress)? {
            Some(deployed) => deployed,
            None => self.build(plan, &source.root, progress)?,
        };
        // In Modpack mode the whole pack is assembled in the App Data Store before Sync
        // runs — rule 7's ordering again: the game is untouched until everything that can
        // fail has succeeded.
        let staged_modpack = if plan.modpack() {
            let resolved = self.resolve_sources(plan, &source.root)?;
            Some(crate::modpack::assemble(
                plan,
                &resolved,
                &built_dll,
                &self.work_dir,
                self.modpack_assembler.as_ref(),
                progress,
            )?)
        } else {
            None
        };
        self.sync(
            plan,
            &source.root,
            &built_dll,
            staged_modpack.as_deref(),
            &fingerprint,
            progress,
        )
    }

    /// The deployed DLL, brought back into the build directory — if and only if the recorded
    /// fingerprint matches `fingerprint` and the DLL still hashes to what the record
    /// promises. `None` means: build.
    fn reusable_deployed_dll(
        &self,
        plan: &Plan,
        fingerprint: &BuildFingerprint,
        progress: &ProgressReporter,
    ) -> Result<Option<PathBuf>, InstallError> {
        // Both homes a deployed DLL can have — the MODS install and inside a Modpack. The
        // DLL is identical between the two modes (the mode changes packaging, not compile
        // inputs), so either record can prove a rebuild unnecessary — which is what makes
        // switching mode cheap.
        for folder in [
            deployed_dll_home(plan, crate::InstallMode::Mods),
            deployed_dll_home(plan, crate::InstallMode::Modpack),
        ] {
            let Ok(sidecar) = std::fs::read_to_string(folder.join(FINGERPRINT_FILE_NAME)) else {
                continue;
            };
            let Some(promised_hash) = fingerprint.matches_sidecar(&sidecar) else {
                continue;
            };
            let deployed = folder.join(BUILT_DLL_FILE_NAME);
            if fnv1a64_of_file(&deployed) != Some(promised_hash) {
                continue;
            }

            // Copied out of the game folder before Sync starts deleting, so the skip path
            // feeds Sync exactly the way a build would (rule 7's ordering is preserved:
            // nothing in the game is touched yet).
            let build_dir = self.work_dir.join("build");
            tree::create_dir_all(&build_dir)?;
            let output_path = build_dir.join(BUILT_DLL_FILE_NAME);
            tree::copy_file(&deployed, &output_path)?;
            progress.report(
                Stage::Build,
                "The DLL is already up to date — build skipped.",
            );
            return Ok(Some(output_path));
        }
        Ok(None)
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

    fn fetch(
        &self,
        plan: &Plan,
        progress: &ProgressReporter,
    ) -> Result<MaterializedSource, InstallError> {
        progress.report(Stage::Fetch, "Getting the mod files ready.");
        let source = self
            .source_provider
            .materialize(&plan.configuration.source, progress)
            .map_err(InstallError::Fetch)?;

        // Check everything the plan needs is actually there before the build burns minutes —
        // and, more importantly, before Sync starts deleting (rule 7).
        let resolved = self.resolve_sources(plan, &source.root)?;

        // Dev mode only: hold the checkout against each mod's own .modinfo manifest. A
        // listed-but-gone file fails here; unlisted extras warn and continue (the split is
        // explained at `validate_dev_manifest`). Upstream Versions are not checked — they
        // ship what upstream released, and the player cannot act on a difference.
        if matches!(
            plan.configuration.source,
            crate::InstallationSource::LocalRepo { .. }
        ) {
            for (index, path) in &resolved {
                let Some(deployment) = plan.deployments.get(*index) else {
                    continue;
                };
                crate::modinfo::validate_dev_manifest(
                    deployment.claimed.folder_name(),
                    path,
                    progress,
                )?;
            }
        }

        for file in &plan.files {
            let path = source.root.join(file.source_path());
            if !path.is_file() {
                return Err(InstallError::MissingInSource {
                    item: SourceItem::File,
                    name: file.source_path().to_owned(),
                    path,
                });
            }
        }

        progress.report(Stage::Fetch, "Mod files ready.");
        Ok(source)
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
            build_configuration: plan.configuration.build_configuration,
            version_label: plan.configuration.source.version_label(),
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
        staged_modpack: Option<&Path>,
        fingerprint: &BuildFingerprint,
        progress: &ProgressReporter,
    ) -> Result<InstallOutcome, InstallError> {
        progress.report(Stage::Sync, "Installing into the game.");

        // Resolved before a single deletion, not between them. `fetch` has already checked the
        // same thing, but Sync must not depend on that: the moment this runs after a removal
        // it becomes a way for a Deployment to stop half-done, which is what rule 7 forbids.
        let sources = self.resolve_sources(plan, source_root)?;

        let mut removed = Vec::new();
        for folder in plan.removed_folders() {
            for path in folder.every_path_in(&plan.folders) {
                if !path.exists() {
                    continue;
                }
                tree::remove_if_present(&path)?;
                if !removed.contains(&folder) {
                    // One line per folder, not one per name it has gone by: a player reading
                    // the log does not care that it was found under an older spelling.
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
        }

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

        let mut deployed = Vec::new();
        for (index, from) in sources {
            let Some(deployment) = plan.deployments.get(index) else {
                continue;
            };
            // In Modpack mode the MODS-target folders were staged inside the pack instead
            // of deploying here — and, deliberately, whatever is in MODS stays (ticket 11).
            if !plan.deploys_directly(deployment) {
                continue;
            }
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

        // The assembled Modpack, deployed whole. Verbatim: the stage is the Core's own
        // work, Built DLL included, so the source-tree exclusions must not apply.
        if let Some(stage) = staged_modpack {
            let destination = ClaimedFolder::Modpack.path_in(&plan.folders);
            tree::remove_if_present(&destination)?;
            tree::copy_all(stage, &destination)?;
            progress.report(
                Stage::Sync,
                format!("Installed {}.", ClaimedFolder::Modpack.folder_name()),
            );
            deployed.push(ClaimedFolder::Modpack);
            deployed.sort_unstable();
        }

        for file in &plan.files {
            tree::copy_file(
                &source_root.join(file.source_path()),
                &file.path_in(&plan.folders),
            )?;
            progress.report(Stage::Sync, format!("Installed {}.", file.file_name()));
        }

        // Every Flavor includes the Community Patch, and the Built DLL is the only DLL
        // deployed. In Mods mode it goes at the root of `(1) Community Patch`; in Modpack
        // mode the assembly already placed it inside the pack, so it is deployed by now
        // either way and only the sidecar's home differs.
        let dll_home = deployed_dll_home(plan, plan.configuration.install_mode);
        let dll_destination = dll_home.join(BUILT_DLL_FILE_NAME);
        if staged_modpack.is_none() {
            tree::copy_file(built_dll, &dll_destination)?;
            progress.report(Stage::Sync, "Installed the DLL.");
        }

        // The Build Fingerprint sidecar, beside the DLL it describes (both inside a Claimed
        // Folder, so rule 6 holds). Hashed from the deployed copy, so what is recorded is
        // what a later launch will re-hash. A failure on this path only ever costs a
        // rebuild, never a false skip, so it does not fail an otherwise complete
        // Deployment — but it is said out loud (rule 11): "it rebuilds every time" must be
        // diagnosable from what the user can show us.
        let sidecar = dll_home.join(FINGERPRINT_FILE_NAME);
        let recorded = match fnv1a64_of_file(&dll_destination) {
            Some(hash) => std::fs::write(&sidecar, fingerprint.sidecar_contents(hash)).is_ok(),
            None => false,
        };
        if !recorded {
            progress.report(
                Stage::Sync,
                format!(
                    "Could not record the build fingerprint at {} — the next install will \
                     rebuild instead of skipping.",
                    sidecar.display()
                ),
            );
        }

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
                if !removed.contains(&folder) {
                    progress.report(Stage::Sync, format!("Removed {}.", folder.folder_name()));
                    removed.push(folder);
                }
            }
        }

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

/// Where the deployed DLL (and its fingerprint sidecar) lives for a given install mode:
/// the root of `(1) Community Patch` — in MODS, or inside the Modpack.
fn deployed_dll_home(plan: &Plan, mode: crate::InstallMode) -> PathBuf {
    match mode {
        crate::InstallMode::Mods => ClaimedFolder::CommunityPatch.path_in(&plan.folders),
        crate::InstallMode::Modpack => ClaimedFolder::Modpack
            .path_in(&plan.folders)
            .join("Mods")
            .join(ClaimedFolder::CommunityPatch.folder_name()),
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
