//! Turning an Install Configuration into the list of folder operations it implies.

use crate::claimed::{ClaimedFolder, GameFolders};
use crate::configuration::{Flavor, InstallConfiguration};
use crate::error::{GameFolderProblem, InstallError};

/// One Claimed Folder and the folder in the Installation Source it is filled from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderDeployment {
    pub(crate) claimed: ClaimedFolder,
    /// Path relative to the source root. Today always the Claimed Folder's own name; the
    /// EUI swaps in ticket 02 are the first case where the two differ.
    pub(crate) source_subdir: String,
}

/// What a Deployment is going to do. Produced by [`crate::Core::plan`], executed by
/// [`crate::Core::execute`].
#[derive(Debug, Clone)]
pub struct Plan {
    pub(crate) configuration: InstallConfiguration,
    pub(crate) folders: GameFolders,
    pub(crate) deployments: Vec<FolderDeployment>,
}

impl Plan {
    pub(crate) fn build(
        configuration: &InstallConfiguration,
        folders: &GameFolders,
    ) -> Result<Self, InstallError> {
        // Before anything else. Rule 6 holds only if the roots Sync derives its paths from are
        // real absolute locations — a relative or empty root would aim the deletes and copies
        // at the process's working directory instead of at the game.
        for (which, path) in [
            ("MODS", &folders.mods),
            ("DLC", &folders.dlc),
            ("Text", &folders.text),
        ] {
            let problem = if path.as_os_str().is_empty() {
                Some(GameFolderProblem::NotChosen)
            } else if !path.is_absolute() {
                Some(GameFolderProblem::NotAbsolute)
            } else if !path.is_dir() {
                Some(GameFolderProblem::NotADirectory)
            } else {
                None
            };
            if let Some(problem) = problem {
                return Err(InstallError::UnusableGameFolder {
                    which,
                    path: path.clone(),
                    problem,
                });
            }
        }

        let deployments = match &configuration.flavor {
            Flavor::CommunityPatch => vec![FolderDeployment {
                claimed: ClaimedFolder::CommunityPatch,
                source_subdir: ClaimedFolder::CommunityPatch.folder_name().to_owned(),
            }],
            // The walking skeleton deploys the Community Patch only. Ticket 02 fills in the
            // rest of the matrix — Vox Populi, EUI, 43 Civs, Squads, VPUI, the tips XML —
            // against the InnoSetup script as its behavioural reference. Failing here keeps
            // the gap visible instead of silently deploying half an install.
            Flavor::VoxPopuli { .. } => {
                return Err(InstallError::UnsupportedConfiguration {
                    message: "This build of the installer can only install Community Patch. \
                              Vox Populi is not available yet."
                        .to_owned(),
                    detail: "plan: Flavor::VoxPopuli is not implemented until ticket 02 \
                             (full deployment matrix + Sync semantics)"
                        .to_owned(),
                });
            }
        };

        Ok(Self {
            configuration: configuration.clone(),
            folders: folders.clone(),
            deployments,
        })
    }

    /// The Claimed Folders this Deployment will create or refresh, in a fixed order.
    pub(crate) fn deployed_folders(&self) -> Vec<ClaimedFolder> {
        let mut folders: Vec<_> = self.deployments.iter().map(|d| d.claimed).collect();
        folders.sort_unstable();
        folders
    }

    /// The Claimed Folders that do not belong to this configuration and will be removed if
    /// they are present. This is the Sync half that keeps a switched configuration clean.
    pub(crate) fn removed_folders(&self) -> Vec<ClaimedFolder> {
        let deployed = self.deployed_folders();
        ClaimedFolder::ALL
            .into_iter()
            .filter(|folder| !deployed.contains(folder))
            .collect()
    }
}
