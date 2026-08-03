//! The exact set of folders and files the installer owns, and where they live in the game.

use std::path::{Path, PathBuf};

use crate::error::{GameFolderProblem, InstallError};

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

impl GameFolders {
    /// The Documents-side root — the `Sid Meier's Civilization 5` folder holding `MODS`,
    /// `Text`, `ModUserData`, `UserSettings.ini` and `cache`.
    ///
    /// Derived rather than stored, and deliberately only when the MODS and Text Folders agree
    /// on it. That agreement is checked before a Deployment runs, so by the time Sync asks for
    /// the `cache` folder the answer is a real location and not a guess — which matters,
    /// because clearing `cache` is the one write rule 6 permits outside a Claimed Folder.
    pub(crate) fn documents_root(&self) -> Option<&Path> {
        match (self.mods.parent(), self.text.parent()) {
            (Some(from_mods), Some(from_text)) if from_mods == from_text => Some(from_mods),
            _ => None,
        }
    }

    /// The game's `cache` folder, whose contents are cleared after every Deployment.
    ///
    /// `ModUserData` sits beside it and is never touched — that is the whole reason this
    /// returns one specific child rather than the Documents root itself.
    pub(crate) fn cache(&self) -> Option<PathBuf> {
        self.documents_root().map(|root| root.join("cache"))
    }

    /// Refuse folders the installer cannot safely write to, before anything happens.
    ///
    /// Rule 6 holds only if the roots every destination path is derived from are real absolute
    /// locations: a relative or empty root would aim Sync's deletes and copies at whatever the
    /// working directory happens to be. Both Deployment and Uninstall start here, because both
    /// delete things.
    pub(crate) fn check(&self) -> Result<(), InstallError> {
        for (which, path) in [
            ("MODS", &self.mods),
            ("DLC", &self.dlc),
            ("Text", &self.text),
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

        // Only once all three are real directories is it worth asking whether the Documents
        // side hangs together. Without this the `cache` folder Sync clears would be a guess.
        if self.documents_root().is_none() {
            return Err(InstallError::UnusableGameFolder {
                which: "Text",
                path: self.text.clone(),
                problem: GameFolderProblem::NotBesideTheOthers,
            });
        }

        Ok(())
    }
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

/// A single file the installer owns, deployed beside content it does not own.
///
/// The Claimed *Folders* are replaced wholesale, which is what makes Sync exact. That is not
/// available in the Text Folder: it belongs to the game and holds the player's other text
/// files, so the installer has to name the one file in it that is its own. Rule 6 still holds
/// — the path is derived here and never from anything a caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClaimedFile {
    /// The loading-screen tips, deployed by every Vox Populi configuration.
    VpuiTips,
}

impl ClaimedFile {
    /// Every Claimed File, in a fixed order (rule 8).
    pub const ALL: [Self; 1] = [Self::VpuiTips];

    /// The file's name in the game folder.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::VpuiTips => "VPUI_tips_en_us.xml",
        }
    }

    /// Where it comes from, relative to the Installation Source root.
    ///
    /// Note the space in `VPUI Text`, and that it is a different folder from the `VPUI` that
    /// is deployed as DLC.
    pub(crate) fn source_path(self) -> &'static str {
        match self {
            Self::VpuiTips => "VPUI Text/VPUI_tips_en_us.xml",
        }
    }

    /// Where it lives in the user's game.
    pub fn path_in(self, folders: &GameFolders) -> PathBuf {
        match self {
            Self::VpuiTips => folders.text.join(self.file_name()),
        }
    }
}
