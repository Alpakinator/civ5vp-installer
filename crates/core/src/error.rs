//! Typed errors. Every one carries a sentence for the user and the detail for the log.

use std::fmt;
use std::path::PathBuf;

use crate::boundaries::BoundaryError;

/// Why a game folder cannot be used as a Deployment target.
///
/// Rule 6 says every path the installer writes to is derived from a Claimed Folder root. That
/// only holds if the root itself is a real, absolute location: a relative or empty path would
/// send Sync's deletes and copies at whatever the working directory happens to be.
///
/// This is the safety floor, not folder detection — ticket 03 adds the marker checks
/// (`CivilizationV.exe`, `Assets/DLC/Expansion2/`, `UserSettings.ini`) that decide whether a
/// folder really is the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFolderProblem {
    /// No path at all.
    NotChosen,
    /// A relative path, which would be resolved against the working directory.
    NotAbsolute,
    /// Nothing there, or there but not a directory.
    NotADirectory,
    /// The MODS and Text Folders are not in the same `Sid Meier's Civilization 5` folder.
    ///
    /// In a real install they always are, and the installer relies on it: the game's `cache`
    /// folder is their sibling, and clearing it is the one write rule 6 allows outside a
    /// Claimed Folder. Two folders that disagree about where the game is mean the installer
    /// cannot say which `cache` is the right one, so it refuses rather than guessing.
    NotBesideTheOthers,
}

/// What the Installation Source was missing.
///
/// The three read very differently to someone who is not a programmer, so they are separated
/// here rather than papered over with one sentence (rule 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceItem {
    /// A whole folder the configuration deploys.
    Folder,
    /// A single file the configuration deploys.
    File,
    /// A folder that is present, but holds none of the files this configuration takes from it.
    Contents,
}

/// Anything that can stop a Deployment.
///
/// Rule 10: `user_message` is what the UI shows — a sentence a non-programmer can act on.
/// `log_detail` is what goes in the log file, and is where raw compiler, git, and IO text
/// is allowed to appear.
#[derive(Debug)]
pub enum InstallError {
    /// The source provider could not materialize the Installation Source.
    Fetch(BoundaryError),
    /// The toolchain runner could not produce the Built DLL.
    Build(BoundaryError),
    /// The toolchain runner reported success but wrote no DLL.
    MissingBuiltDll { expected: PathBuf },
    /// A file operation against the game folders failed.
    Deployment {
        /// What was being attempted, e.g. "copy" — used to build the log line.
        action: &'static str,
        path: PathBuf,
        cause: std::io::Error,
    },
    /// The Installation Source does not contain something the configuration needs.
    MissingInSource {
        item: SourceItem,
        /// The name to show the user — a folder name, or a path relative to the source root.
        name: String,
        path: PathBuf,
    },
    /// A game folder the installer cannot safely write to.
    UnusableGameFolder {
        /// Which one, in the user's words: "MODS", "DLC", "Text".
        which: &'static str,
        path: PathBuf,
        problem: GameFolderProblem,
    },
    /// A configuration this build of the installer cannot deploy yet.
    UnsupportedConfiguration { message: String, detail: String },
}

impl InstallError {
    /// The sentence shown in the UI.
    pub fn user_message(&self) -> String {
        match self {
            Self::Fetch(err) => err.message().to_owned(),
            Self::Build(err) => format!(
                "{} You can try installing a Release instead, which is the most tested option.",
                err.message()
            ),
            Self::MissingBuiltDll { .. } => {
                "The DLL build finished but produced no file, so nothing was installed. \
                 Your game is unchanged."
                    .to_owned()
            }
            Self::Deployment { path, .. } => format!(
                "Could not write to {}. Check that the folder exists and is not read-only.",
                path.display()
            ),
            Self::MissingInSource { item, name, .. } => match item {
                SourceItem::Folder => format!(
                    "The mod files are missing the \"{name}\" folder, so there is nothing to \
                     install. Your game is unchanged."
                ),
                SourceItem::File => format!(
                    "The mod files are missing \"{name}\", so there is nothing to install. \
                     Your game is unchanged."
                ),
                SourceItem::Contents => format!(
                    "The \"{name}\" folder in the mod files does not contain what this \
                     installer expects, so nothing was installed. Your game is unchanged."
                ),
            },
            Self::UnusableGameFolder {
                which,
                path,
                problem,
            } => match problem {
                GameFolderProblem::NotChosen => {
                    format!("Choose your {which} folder before installing.")
                }
                GameFolderProblem::NotAbsolute => format!(
                    "The {which} folder needs to be a full path starting from the root of the \
                     drive, not \"{}\".",
                    path.display()
                ),
                GameFolderProblem::NotADirectory => format!(
                    "There is no {which} folder at {}. Check the path and try again.",
                    path.display()
                ),
                GameFolderProblem::NotBesideTheOthers => format!(
                    "Your MODS and Text folders should both be inside the same \
                     \"Sid Meier's Civilization 5\" folder, but the {which} folder is at {}. \
                     Pick them again, or let the installer detect them for you.",
                    path.display()
                ),
            },
            Self::UnsupportedConfiguration { message, .. } => message.clone(),
        }
    }

    /// The full detail, for the log file (rule 11).
    pub fn log_detail(&self) -> String {
        match self {
            Self::Fetch(err) => format!("fetch failed: {}", err.detail()),
            Self::Build(err) => format!("build failed: {}", err.detail()),
            Self::MissingBuiltDll { expected } => {
                format!(
                    "toolchain runner returned Ok but {} does not exist",
                    expected.display()
                )
            }
            Self::Deployment {
                action,
                path,
                cause,
            } => format!("deployment: {action} {} failed: {cause}", path.display()),
            Self::MissingInSource { item, name, path } => format!(
                "installation source: {item:?} \"{name}\" missing or empty at {}",
                path.display()
            ),
            Self::UnusableGameFolder {
                which,
                path,
                problem,
            } => format!(
                "{which} folder rejected before planning: {problem:?} at {}",
                path.display()
            ),
            Self::UnsupportedConfiguration { detail, .. } => detail.clone(),
        }
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Deployment { cause, .. } => Some(cause),
            _ => None,
        }
    }
}
