//! The Replaced File: a game-owned file the installer may overwrite, and the copy that
//! makes that reversible.
//!
//! ADR-0006. Everything here exists to keep one promise — a player who uninstalls gets back
//! the file the game shipped with.

use std::path::{Path, PathBuf};

use crate::claimed::GameFolders;
use crate::error::InstallError;
use crate::tree;

/// A file belonging to the game that the installer replaces.
///
/// Deliberately an enum of one rather than a path a caller supplies: the whole reason the
/// Claimed-Folders invariant can be relaxed at all is that the exception is this short and
/// this explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacedFile {
    /// The Lua engine, in the Game Installation root beside the executables.
    LuaEngine,
}

impl ReplacedFile {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::LuaEngine => "lua51_Win32.dll",
        }
    }

    /// Where it lives in the user's game.
    ///
    /// Built from the Game Installation root detection resolved, never from the DLC Folder's
    /// ancestry: a path that decides where a game file is overwritten must not be inferred.
    pub fn path_in(self, folders: &GameFolders) -> PathBuf {
        match self {
            Self::LuaEngine => folders.game_root.join(self.file_name()),
        }
    }
}

/// What a restore did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    FromBackup,
    /// No backup was held — nothing was changed.
    NothingToRestore,
}

/// What a Deployment did to the game's Lua engine.
///
/// Three states rather than a bool, because "the configuration does not want LuaJIT" covers
/// two different situations that must not be reported as one: a Deployment that put the stock
/// engine back, and a Deployment that had nothing to put back because none was ever replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineOutcome {
    /// LuaJIT was installed. The game's own engine is held in the Backup Store.
    Replaced,
    /// The game's own engine was put back, because this Deployment did not ask for LuaJIT
    /// and an earlier one had replaced it.
    Restored,
    /// Neither: no LuaJIT was asked for and none was ever installed.
    Untouched,
}

/// Where the originals are kept, inside the App Data Store.
pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_of(&self, file: ReplacedFile) -> PathBuf {
        self.root.join(file.file_name())
    }

    /// Whether an original is already held.
    pub fn holds(&self, file: ReplacedFile) -> bool {
        self.path_of(file).is_file()
    }

    /// Save the game's copy — but only the first time.
    ///
    /// The guard is the entire point: by the second Deployment the file in the game is the
    /// installer's own replacement, and saving that would destroy the only copy of the
    /// original.
    ///
    /// A missing file in the game is nothing to save and so nothing to report. A player may
    /// have a Steam verify in flight, or an installation that never had the engine where the
    /// installer looks; neither is a reason to stop a Deployment that has not written yet.
    pub fn back_up_once(&self, file: ReplacedFile, from: &Path) -> Result<(), InstallError> {
        if self.holds(file) || !from.is_file() {
            return Ok(());
        }
        tree::create_dir_all(&self.root)?;
        tree::copy_file(from, &self.path_of(file))
    }

    /// Put the original back, if one is held.
    ///
    /// A player who cleared the App Data Store between install and uninstall has no original
    /// left to put back. Uninstall says so and carries on removing everything else, because
    /// failing here would leave the Claimed Folders in the game as well as the replacement.
    pub fn restore(&self, file: ReplacedFile, to: &Path) -> Result<Restored, InstallError> {
        if !self.holds(file) {
            return Ok(Restored::NothingToRestore);
        }
        tree::copy_file(&self.path_of(file), to)?;
        Ok(Restored::FromBackup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole safety property: the *stock* engine is what gets saved. A second
    /// Deployment must not copy LuaJIT over the backup, or uninstall would "restore" the
    /// very thing it is meant to remove.
    #[test]
    fn a_backup_is_taken_once_and_never_overwritten() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let store = BackupStore::new(dir.path().join("backups"));
        let game_file = dir.path().join("lua51_Win32.dll");

        let Ok(()) = std::fs::write(&game_file, b"stock lua 5.1") else {
            unreachable!("the stock engine in the game")
        };
        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the first backup")
        };

        let Ok(()) = std::fs::write(&game_file, b"luajit") else {
            unreachable!("the overwrite a Deployment would do")
        };
        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the second backup is a no-op")
        };

        let Ok(outcome) = store.restore(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the restore")
        };
        assert_eq!(outcome, Restored::FromBackup);
        let Ok(restored) = std::fs::read(&game_file) else {
            unreachable!("the restored engine")
        };
        assert_eq!(
            restored, b"stock lua 5.1",
            "the stock engine must come back, not the replacement"
        );
    }

    /// A player who cleared the App Data Store between install and uninstall has no backup.
    /// That is a thing to report, not a failure — uninstall still removes everything else.
    #[test]
    fn restoring_without_a_backup_says_so_instead_of_failing() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let store = BackupStore::new(dir.path().join("backups"));
        let game_file = dir.path().join("lua51_Win32.dll");
        let Ok(()) = std::fs::write(&game_file, b"luajit") else {
            unreachable!("the replacement in the game")
        };

        let Ok(outcome) = store.restore(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("a restore without a backup must not fail")
        };
        assert_eq!(outcome, Restored::NothingToRestore);
    }

    /// Backing up a file the game does not have must be quiet, not fatal. It happens to a
    /// player whose installation is mid-verify, and the Deployment has written nothing at
    /// that point — refusing to continue would be a failure invented by the installer.
    #[test]
    fn backing_up_a_missing_game_file_is_a_no_op() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let store = BackupStore::new(dir.path().join("backups"));
        let missing = dir.path().join("lua51_Win32.dll");

        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &missing) else {
            unreachable!("a missing game file must not fail the backup")
        };
        assert!(
            !store.holds(ReplacedFile::LuaEngine),
            "nothing was there to hold"
        );
    }

    /// `holds` is what the back-up-once guard is built on, so it is checked directly rather
    /// than only through the behaviour it produces.
    #[test]
    fn the_store_holds_an_original_only_after_one_is_saved() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let store = BackupStore::new(dir.path().join("backups"));
        let game_file = dir.path().join("lua51_Win32.dll");
        let Ok(()) = std::fs::write(&game_file, b"stock lua 5.1") else {
            unreachable!("the stock engine in the game")
        };

        assert!(!store.holds(ReplacedFile::LuaEngine));
        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the backup")
        };
        assert!(store.holds(ReplacedFile::LuaEngine));
    }

    /// The Replaced File is written into the Game Installation root, not the Documents side
    /// — the one place the installer had never written before ADR-0006.
    #[test]
    fn the_engine_sits_in_the_game_installation_root() {
        let folders = GameFolders {
            mods: PathBuf::from("/documents/MODS"),
            dlc: PathBuf::from("/game/Assets/DLC"),
            text: PathBuf::from("/documents/Text"),
            game_root: PathBuf::from("/game"),
        };

        assert_eq!(
            ReplacedFile::LuaEngine.path_in(&folders),
            PathBuf::from("/game/lua51_Win32.dll")
        );
    }
}
