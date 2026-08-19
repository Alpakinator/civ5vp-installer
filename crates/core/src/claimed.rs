//! The exact set of folders and files the installer owns, and where they live in the game.

use std::path::{Path, PathBuf};

use crate::error::{GameFolderProblem, InstallError};

/// The resolved deployment targets. Detection resolves these; the Core is handed them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFolders {
    /// `…/Documents/My Games/Sid Meier's Civilization 5/MODS`
    pub mods: PathBuf,
    /// `…/Sid Meier's Civilization V/Assets/DLC`
    pub dlc: PathBuf,
    /// `…/Documents/My Games/Sid Meier's Civilization 5/Text`
    pub text: PathBuf,
    /// `…/Sid Meier's Civilization V` - the Game Installation root.
    ///
    /// Held rather than derived from the DLC Folder's grandparent, because this is where the
    /// Replaced File is written (ADR-0006). A path that decides where a file belonging to the
    /// game gets overwritten must be one detection resolved, not one Sync inferred.
    pub game_root: PathBuf,
}

impl GameFolders {
    /// The Documents-side root - the `Sid Meier's Civilization 5` folder holding `MODS`,
    /// `Text`, `ModUserData`, `UserSettings.ini` and `cache`.
    ///
    /// Derived rather than stored, and deliberately only when the MODS and Text Folders agree
    /// on it. That agreement is checked before a Deployment runs, so by the time Sync asks for
    /// the `cache` folder the answer is a real location and not a guess - which matters,
    /// because clearing `cache` is the one write permitted outside a Claimed Folder.
    pub(crate) fn documents_root(&self) -> Option<&Path> {
        match (self.mods.parent(), self.text.parent()) {
            (Some(from_mods), Some(from_text)) if from_mods == from_text => Some(from_mods),
            _ => None,
        }
    }

    /// The game's `cache` folder, whose contents are cleared after every Deployment.
    ///
    /// `ModUserData` sits beside it and is never touched - that is the whole reason this
    /// returns one specific child rather than the Documents root itself.
    pub(crate) fn cache(&self) -> Option<PathBuf> {
        self.documents_root().map(|root| root.join("cache"))
    }

    /// Refuse folders the installer cannot safely write to, before anything happens.
    ///
    /// Every destination path is derived from these roots, so they must be real absolute
    /// locations: a relative or empty root would aim Sync's deletes and copies at whatever the
    /// working directory happens to be. Both Deployment and Uninstall start here, because both
    /// delete things.
    pub(crate) fn check(&self) -> Result<(), InstallError> {
        for (which, path) in [
            ("MODS", &self.mods),
            ("DLC", &self.dlc),
            ("Text", &self.text),
            ("Game Installation", &self.game_root),
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
/// This is the whole list. Nothing outside the Claimed Folders is ever written, moved, or
/// deleted; that holds because every destination path is derived from
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
    /// The generated Modpack. Unlike every other Claimed Folder it is not filled
    /// from the Installation Source - the Core assembles it in the App Data Store and Sync
    /// deploys the result.
    Modpack,
}

impl ClaimedFolder {
    /// Every Claimed Folder, in a fixed order. Iterating this - rather than a `HashSet` - is
    /// what keeps Sync's file operations deterministic.
    pub const ALL: [Self; 8] = [
        Self::CommunityPatch,
        Self::VoxPopuli,
        Self::EuiCompatibilityFiles,
        Self::FortyThreeCivsCommunityPatch,
        Self::SquadsForVoxPopuli,
        Self::Vpui,
        Self::UiBc1,
        Self::Modpack,
    ];

    /// Every name this folder has been known by, the one in current use first.
    ///
    /// Upstream renames these. `(3a)` was `(3a) EUI Compatibility Files` up to and including
    /// `Release-5.1`, and `(3a) VP - EUI Compatibility Files` from `Release-5.2` onwards. The
    /// installer offers every one of those Versions, so a single hardcoded name would make an
    /// older Release fail with "the mod files are missing …" for a folder that is right there
    /// under its old name.
    ///
    /// Every name here is Claimed. That matters as much for removal as for deployment: a player
    /// who installs an old Release and then a new one must not be left with both folders, which
    /// is exactly the stale-install corruption Sync exists to prevent.
    pub fn folder_names(self) -> &'static [&'static str] {
        match self {
            Self::CommunityPatch => &["(1) Community Patch"],
            Self::VoxPopuli => &["(2) Vox Populi"],
            Self::EuiCompatibilityFiles => &[
                "(3a) VP - EUI Compatibility Files",
                "(3a) EUI Compatibility Files",
            ],
            Self::FortyThreeCivsCommunityPatch => &["(3b) 43 Civs Community Patch"],
            Self::SquadsForVoxPopuli => &["(4a) Squads for VP"],
            Self::Vpui => &["VPUI"],
            Self::UiBc1 => &["UI_bc1"],
            // The name is the Community Patch DLL's own (`CvGame::CreateMPMP`), and the
            // in-game Modpack Maker writes the same folder - so a Deployment that removes
            // this also cleans up a modpack the player made by hand, which is exactly the
            // conflict removal exists to prevent.
            Self::Modpack => &["VP_MODPACK"],
        }
    }

    /// The name this folder is deployed under: the current one, whatever the source called it.
    pub fn folder_name(self) -> &'static str {
        // `folder_names` is never empty - every arm above is a non-empty literal - so this
        // cannot fall through. The crate denies `unwrap`, hence the explicit arm.
        match self.folder_names() {
            [current, ..] => current,
            [] => "",
        }
    }

    pub fn target(self) -> DeploymentTarget {
        match self {
            Self::CommunityPatch
            | Self::VoxPopuli
            | Self::EuiCompatibilityFiles
            | Self::FortyThreeCivsCommunityPatch
            | Self::SquadsForVoxPopuli => DeploymentTarget::ModsFolder,
            Self::Vpui | Self::UiBc1 | Self::Modpack => DeploymentTarget::DlcFolder,
        }
    }

    /// Where this Claimed Folder lives in the user's game.
    pub fn path_in(self, folders: &GameFolders) -> PathBuf {
        self.root_in(folders).join(self.folder_name())
    }

    /// Every place in the game this folder could be, old names included. What Sync removes.
    pub(crate) fn every_path_in(self, folders: &GameFolders) -> Vec<PathBuf> {
        let root = self.root_in(folders);
        self.folder_names().iter().map(|n| root.join(n)).collect()
    }

    fn root_in(self, folders: &GameFolders) -> &Path {
        match self.target() {
            DeploymentTarget::ModsFolder => &folders.mods,
            DeploymentTarget::DlcFolder => &folders.dlc,
        }
    }
}

/// A single file the installer owns, deployed beside content it does not own.
///
/// The Claimed *Folders* are replaced wholesale, which is what makes Sync exact. That is not
/// available in the Text Folder: it belongs to the game and holds the player's other text
/// files, so the installer has to name the one file in it that is its own. The path is
/// still derived here and never from anything a caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClaimedFile {
    /// The loading-screen tips, deployed by every Vox Populi configuration.
    VpuiTips,
}

impl ClaimedFile {
    /// Every Claimed File, in a fixed order.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Three real directories and one caller-supplied Game Installation root, so a test can
    /// vary the fourth without the first three failing the check ahead of it.
    fn folders_with_game_root(root: &Path, game_root: PathBuf) -> Option<GameFolders> {
        for name in ["MODS", "DLC", "Text"] {
            std::fs::create_dir_all(root.join(name)).ok()?;
        }
        Some(GameFolders {
            mods: root.join("MODS"),
            dlc: root.join("DLC"),
            text: root.join("Text"),
            game_root,
        })
    }

    /// The Game Installation root decides where the Replaced File is written, so it is held
    /// to exactly the standard the other three are - an unset one must stop a Deployment
    /// before it starts, not surface as a write to a relative path.
    #[test]
    fn an_unset_game_root_is_refused() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let Some(folders) = folders_with_game_root(dir.path(), PathBuf::new()) else {
            unreachable!("the fixture directories")
        };

        let Err(InstallError::UnusableGameFolder { which, problem, .. }) = folders.check() else {
            unreachable!("an unset Game Installation root must be refused")
        };
        assert_eq!(which, "Game Installation");
        assert_eq!(problem, GameFolderProblem::NotChosen);
    }

    /// A relative root would aim the Replaced File's write at whatever the working directory
    /// happens to be.
    #[test]
    fn a_relative_game_root_is_refused() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let Some(folders) = folders_with_game_root(dir.path(), PathBuf::from("game")) else {
            unreachable!("the fixture directories")
        };

        let Err(InstallError::UnusableGameFolder { which, problem, .. }) = folders.check() else {
            unreachable!("a relative Game Installation root must be refused")
        };
        assert_eq!(which, "Game Installation");
        assert_eq!(problem, GameFolderProblem::NotAbsolute);
    }

    /// The ordinary case still passes, so the new check cannot be satisfied by refusing
    /// everything.
    #[test]
    fn a_real_game_root_is_accepted() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let Some(folders) = folders_with_game_root(dir.path(), dir.path().to_path_buf()) else {
            unreachable!("the fixture directories")
        };

        assert!(folders.check().is_ok());
    }
}
