//! Turning an Install Configuration into the list of folder operations it implies.

use crate::claimed::{ClaimedFile, ClaimedFolder, DeploymentTarget, GameFolders};
use crate::configuration::{Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallMode};
use crate::error::InstallError;

/// The top-level `LUA` folder of `(1)` and `(2)`.
///
/// EUI ships its own Lua in `(3a)`, so when EUI is on these have to go — otherwise the game
/// loads both and the interface breaks. The official installer expresses this by excluding
/// `\LUA` from the bulk copy and adding it back only for the non-EUI configurations; the
/// leading backslash anchors it to the top level, so a nested `LUA` folder deeper in the tree
/// is untouched. `SourceSelection::Without` has the same top-level-only meaning.
const LUA: &[&str] = &["LUA"];

/// What `(3b) 43 Civs Community Patch` ships when 43 Civs is on: its modinfo and one Lua file
/// out of a folder that also holds a DLL. The DLL is deliberately left behind — the 43-civ
/// build is deployed into `(1)`, which is where the modinfo's `OnGetDLLPath` hook looks.
const FORTY_THREE_CIVS_FILES: &[&str] = &["*.modinfo", "AdvancedSetup.lua"];

/// Which part of a source folder a Deployment takes.
///
/// Two configurations deploy the same folder differently rather than deploying a different
/// folder, and this is that difference. Keeping it in the Plan means Sync stays a dumb
/// executor: every decision about what belongs in a Claimed Folder is made before a single
/// file moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceSelection {
    /// The whole folder, minus the standard exclusions.
    Everything,
    /// The whole folder minus these top-level entries. EUI replaces the Lua in `(1)` and `(2)`
    /// with its own, so the originals have to go or the game loads both.
    Without(&'static [&'static str]),
    /// Only these top-level entries. `(3b)` ships as two files out of a much larger folder
    /// when 43 Civs is on.
    ///
    /// An entry beginning `*.` matches every top-level file with that extension. That exists
    /// for one reason: `(3b)`'s modinfo carries its mod version in its file name, so the
    /// official installer's hardcoded `(v 1)` breaks the moment upstream bumps it. Matching
    /// by extension survives that.
    Only(&'static [&'static str]),
}

impl SourceSelection {
    /// Is this top-level entry of the source folder part of the Deployment?
    pub(crate) fn admits(&self, name: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Without(excluded) => !excluded.iter().any(|e| name.eq_ignore_ascii_case(e)),
            Self::Only(wanted) => wanted.iter().any(|entry| match entry.strip_prefix("*.") {
                Some(extension) => name
                    .rsplit_once('.')
                    .is_some_and(|(_, found)| found.eq_ignore_ascii_case(extension)),
                None => name.eq_ignore_ascii_case(entry),
            }),
        }
    }
}

/// One Claimed Folder, the folder in the Installation Source it is filled from, and how much
/// of that folder is taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderDeployment {
    pub(crate) claimed: ClaimedFolder,
    pub(crate) selection: SourceSelection,
}

impl FolderDeployment {
    /// The names to look for in the Installation Source, in order of preference.
    ///
    /// More than one, because upstream renames folders: which name a given Version uses is a
    /// fact about that Version, so it is settled by looking, not by the Plan (see
    /// [`ClaimedFolder::folder_names`]).
    pub(crate) fn source_candidates(&self) -> &'static [&'static str] {
        self.claimed.folder_names()
    }
}

/// What a Deployment is going to do. Produced by [`crate::Core::plan`], executed by
/// [`crate::Core::execute`].
#[derive(Debug, Clone)]
pub struct Plan {
    pub(crate) configuration: InstallConfiguration,
    pub(crate) folders: GameFolders,
    pub(crate) deployments: Vec<FolderDeployment>,
    pub(crate) files: Vec<ClaimedFile>,
}

impl Plan {
    pub(crate) fn build(
        configuration: &InstallConfiguration,
        folders: &GameFolders,
    ) -> Result<Self, InstallError> {
        // Before anything else: refuse folders the installer cannot safely write to.
        folders.check()?;

        // A Debug DLL is a mod developer's tool. Outside Dev mode there is no way to choose
        // it, and a remembered or hand-edited configuration that says otherwise is refused
        // here rather than quietly built (rule 3: the shell draws choices, the Core rules on
        // them).
        if configuration.build_configuration == crate::BuildConfiguration::Debug
            && !matches!(
                configuration.source,
                crate::InstallationSource::LocalRepo { .. }
            )
        {
            return Err(InstallError::UnsupportedConfiguration {
                message: "Debug builds are only available in Dev mode, building from your \
                          own checkout. Pick Release, or point the installer at a local \
                          Community-Patch-DLL folder."
                    .to_owned(),
                detail: "build_configuration = Debug with a non-LocalRepo source".to_owned(),
            });
        }

        Ok(Self {
            deployments: deployments_for(configuration),
            files: files_for(configuration),
            configuration: configuration.clone(),
            folders: folders.clone(),
        })
    }

    /// The Claimed Files that do not belong to this configuration and will be removed if
    /// they are present.
    pub(crate) fn removed_files(&self) -> Vec<ClaimedFile> {
        ClaimedFile::ALL
            .into_iter()
            .filter(|file| !self.files.contains(file))
            .collect()
    }

    /// Is this a Modpack Deployment (ticket 11)?
    pub(crate) fn modpack(&self) -> bool {
        self.configuration.install_mode == InstallMode::Modpack
    }

    /// Does this deployment go straight into the game, rather than into the Modpack stage?
    ///
    /// In Mods mode, every deployment does. In Modpack mode the MODS-target folders are
    /// staged inside the Modpack instead, while the DLC-target folders (VPUI, EUI's
    /// `UI_bc1`) are still real DLC and deploy as usual.
    pub(crate) fn deploys_directly(&self, deployment: &FolderDeployment) -> bool {
        !self.modpack() || deployment.claimed.target() == DeploymentTarget::DlcFolder
    }

    /// The Claimed Folders this Deployment will create or refresh in the game, in a fixed
    /// order. In Modpack mode that includes the Modpack itself and not the folders staged
    /// inside it.
    pub(crate) fn deployed_folders(&self) -> Vec<ClaimedFolder> {
        let mut folders: Vec<_> = self
            .deployments
            .iter()
            .filter(|d| self.deploys_directly(d))
            .map(|d| d.claimed)
            .collect();
        if self.modpack() {
            folders.push(ClaimedFolder::Modpack);
        }
        folders.sort_unstable();
        folders
    }

    /// The Claimed Folders that do not belong to this configuration and will be removed if
    /// they are present. This is the Sync half that keeps a switched configuration clean.
    ///
    /// Two asymmetric rules from ticket 11, both deliberate:
    /// - A Mods-mode Deployment removes the Modpack. A baked-in Modpack loads at every
    ///   startup, so activating the same mods on top of it corrupts the game.
    /// - A Modpack-mode Deployment does *not* remove the MODS folders. Inactive mods in the
    ///   Mods menu conflict with nothing, and deleting them would destroy a working install
    ///   the player may want to keep.
    pub(crate) fn removed_folders(&self) -> Vec<ClaimedFolder> {
        let deployed = self.deployed_folders();
        ClaimedFolder::ALL
            .into_iter()
            .filter(|folder| !deployed.contains(folder))
            .filter(|folder| !(self.modpack() && folder.target() == DeploymentTarget::ModsFolder))
            .collect()
    }
}

/// The deployment matrix.
///
/// This function is the whole of "what does this configuration install", and it is the only
/// place that knows. Its reference is the official InnoSetup script, `VPSetupData.iss` in
/// `LoneGazebo/Community-Patch-DLL` — the spec names that script as the behavioural authority
/// for file placement. That script models six mutually exclusive components; the same six fall
/// out of the two axes below, which is the cross-check that this table is complete:
///
/// | Flavor          | EUI | 43 Civs | InnoSetup component |
/// | --------------- | --- | ------- | ------------------- |
/// | Community Patch | —   | off     | `Core`              |
/// | Community Patch | —   | on      | `Civ43CPOnly`       |
/// | Vox Populi      | off | off     | `FullNoEUI`         |
/// | Vox Populi      | on  | off     | `FullEUI`           |
/// | Vox Populi      | off | on      | `Civ43NoEUI`        |
/// | Vox Populi      | on  | on      | `Civ43EUI`          |
///
/// There is no seventh row: EUI with Community Patch only is the one illegal combination, and
/// [`Flavor`] makes it unrepresentable rather than rejecting it here.
fn deployments_for(configuration: &InstallConfiguration) -> Vec<FolderDeployment> {
    let eui = match configuration.flavor {
        Flavor::VoxPopuli { eui } => eui,
        Flavor::CommunityPatch => Eui::Disabled,
    };
    // With EUI on, `(3a)` supplies the Lua for `(1)` and `(2)`, so theirs is left behind.
    let base_selection = match eui {
        Eui::Enabled => SourceSelection::Without(LUA),
        Eui::Disabled => SourceSelection::Everything,
    };

    // Every Flavor includes the Community Patch — Vox Populi implies it.
    let mut deployments = vec![at_own_name(
        ClaimedFolder::CommunityPatch,
        base_selection.clone(),
    )];

    if let Flavor::VoxPopuli { .. } = configuration.flavor {
        deployments.push(at_own_name(ClaimedFolder::VoxPopuli, base_selection));
        // Squads and VPUI come with Vox Populi and are never a user-facing choice.
        deployments.push(at_own_name(
            ClaimedFolder::SquadsForVoxPopuli,
            SourceSelection::Everything,
        ));
        deployments.push(at_own_name(
            ClaimedFolder::Vpui,
            SourceSelection::Everything,
        ));
    }

    if eui == Eui::Enabled {
        deployments.push(at_own_name(
            ClaimedFolder::EuiCompatibilityFiles,
            SourceSelection::Everything,
        ));
        deployments.push(at_own_name(
            ClaimedFolder::UiBc1,
            SourceSelection::Everything,
        ));
    }

    if configuration.forty_three_civs == FortyThreeCivs::Enabled {
        deployments.push(at_own_name(
            ClaimedFolder::FortyThreeCivsCommunityPatch,
            SourceSelection::Only(FORTY_THREE_CIVS_FILES),
        ));
    }

    deployments
}

/// The Claimed Files this configuration installs.
///
/// One entry today: every Vox Populi configuration deploys the loading-screen tips into the
/// Text Folder, and no Community-Patch-only configuration does.
fn files_for(configuration: &InstallConfiguration) -> Vec<ClaimedFile> {
    match configuration.flavor {
        Flavor::VoxPopuli { .. } => vec![ClaimedFile::VpuiTips],
        Flavor::CommunityPatch => Vec::new(),
    }
}

/// A Claimed Folder filled from the folder of the same name at the source root.
///
/// That is every one of them: the DLC folders `VPUI` and `UI_bc1` sit at the repository root
/// beside the mod folders, not under a `DLC` directory. "The same name" means any of the names
/// that folder has gone by — which one a Version uses is settled when the source is in hand.
fn at_own_name(claimed: ClaimedFolder, selection: SourceSelection) -> FolderDeployment {
    FolderDeployment { claimed, selection }
}
