//! The exact set of folders the installer owns, and the game folders they live in.

use std::path::{Path, PathBuf};

/// The resolved deployment targets. Ticket 03 detects these; the Core is handed them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFolders {
    /// `…/Documents/My Games/Sid Meier's Civilization 5/MODS`
    pub mods: PathBuf,
    /// `…/Sid Meier's Civilization V/Assets/DLC`
    pub dlc: PathBuf,
    /// `…/Documents/My Games/Sid Meier's Civilization 5/Text`
    pub text: PathBuf,
}

/// Which game folder a Claimed Folder is deployed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentTarget {
    ModsFolder,
    DlcFolder,
}

/// A folder the installer owns and may create, sync, or delete.
///
/// This is the whole list. Rule 6 — nothing outside the Claimed Folders is ever written,
/// moved, or deleted — is upheld by deriving every destination path from
/// [`ClaimedFolder::path_in`] and never from a string the caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClaimedFolder {
    CommunityPatch,
    VoxPopuli,
    EuiCompatibilityFiles,
    FortyThreeCivsCommunityPatch,
    SquadsForVoxPopuli,
    Vpui,
    UiBc1,
}

impl ClaimedFolder {
    /// Every Claimed Folder, in a fixed order. Iterating this — rather than a `HashSet` — is
    /// what keeps Sync's file operations deterministic (rule 8).
    pub const ALL: [Self; 7] = [
        Self::CommunityPatch,
        Self::VoxPopuli,
        Self::EuiCompatibilityFiles,
        Self::FortyThreeCivsCommunityPatch,
        Self::SquadsForVoxPopuli,
        Self::Vpui,
        Self::UiBc1,
    ];

    /// The folder's name on disk, in both the Installation Source and the game folder.
    pub fn folder_name(self) -> &'static str {
        match self {
            Self::CommunityPatch => "(1) Community Patch",
            Self::VoxPopuli => "(2) Vox Populi",
            Self::EuiCompatibilityFiles => "(3a) VP - EUI Compatibility Files",
            Self::FortyThreeCivsCommunityPatch => "(3b) 43 Civs Community Patch",
            Self::SquadsForVoxPopuli => "(4a) Squads for VP",
            Self::Vpui => "VPUI",
            Self::UiBc1 => "UI_bc1",
        }
    }

    pub fn target(self) -> DeploymentTarget {
        match self {
            Self::CommunityPatch
            | Self::VoxPopuli
            | Self::EuiCompatibilityFiles
            | Self::FortyThreeCivsCommunityPatch
            | Self::SquadsForVoxPopuli => DeploymentTarget::ModsFolder,
            Self::Vpui | Self::UiBc1 => DeploymentTarget::DlcFolder,
        }
    }

    /// Where this Claimed Folder lives in the user's game.
    pub fn path_in(self, folders: &GameFolders) -> PathBuf {
        let root: &Path = match self.target() {
            DeploymentTarget::ModsFolder => &folders.mods,
            DeploymentTarget::DlcFolder => &folders.dlc,
        };
        root.join(self.folder_name())
    }
}
