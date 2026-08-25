//! The Replaced File: a game-owned file the installer may overwrite, and the copy that
//! makes that reversible.
//!
//! ADR-0006. Everything here exists to keep one promise - a player who uninstalls gets back
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
    /// Brave New World's main-menu theme.
    ///
    /// Only Expansion2's copy. Vox Populi requires Brave New World, so that is the theme the
    /// menu actually plays; the base game's and Gods & Kings' copies are never reached and
    /// are left alone rather than replaced on the off chance.
    MenuTheme,
}

impl ReplacedFile {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::LuaEngine => "lua51_Win32.dll",
            Self::MenuTheme => "OpeningMenu_Exp2.wav",
        }
    }

    /// What to call this file in a sentence a player reads.
    pub fn describe(self) -> &'static str {
        match self {
            Self::LuaEngine => "Lua engine",
            Self::MenuTheme => "main menu theme",
        }
    }

    /// Where it lives in the user's game.
    ///
    /// Built from the Game Installation root detection resolved, never from the DLC Folder's
    /// ancestry: a path that decides where a game file is overwritten must not be inferred.
    pub fn path_in(self, folders: &GameFolders) -> PathBuf {
        match self {
            Self::LuaEngine => folders.game_root.join(self.file_name()),
            Self::MenuTheme => folders
                .game_root
                .join("Assets/DLC/Expansion2/Sounds/Streamed/Music")
                .join(self.file_name()),
        }
    }

    /// Whether `bytes` are the installer's own replacement rather than the game's file.
    ///
    /// Each Replaced File needs its own answer, and both are structural rather than a hash: a
    /// hash would have to be pinned per game patch and would go stale, while these are true of
    /// any build of the replacement the installer could produce.
    fn is_a_replacement(self, bytes: &[u8]) -> bool {
        match self {
            Self::LuaEngine => looks_like_luajit(bytes),
            Self::MenuTheme => is_silent_wav(bytes),
        }
    }
}

/// What a restore did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    FromBackup,
    /// No backup was held - nothing was changed.
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

/// A canonical PCM WAV header: `RIFF` + `fmt ` + `data`, and nothing else.
pub const WAV_HEADER_LEN: usize = 44;

/// The format Brave New World's `OpeningMenu_Exp2.wav` is in, measured from the shipped file:
/// PCM, stereo, 44100 Hz, 16-bit. The replacement matches it exactly, because the point is a
/// file the game reads the same way - only silent.
const THEME_CHANNELS: u16 = 2;
const THEME_SAMPLE_RATE: u32 = 44_100;
const THEME_BITS: u16 = 16;

/// How long the silent theme runs.
///
/// Not the original's 3.6 minutes, and not a fraction of a second either. A track that ends
/// hands control back to the game's music system, so a very short file would do that many
/// times a minute instead of once every few minutes - harmless in all likelihood, but a
/// change in behaviour that cannot be checked without running the game. A minute keeps the
/// file at ~10 MB while staying the same order of magnitude as the real track.
const THEME_SECONDS: u32 = 60;

/// Build the silent theme: a real WAV in the game's own format, carrying only zeros.
///
/// Generated rather than shipped, so no audio file has to live in the installer binary or be
/// downloaded, and so the header can never drift from the constants above.
pub fn silent_theme() -> Vec<u8> {
    let byte_rate = THEME_SAMPLE_RATE * u32::from(THEME_CHANNELS) * u32::from(THEME_BITS) / 8;
    let block_align = THEME_CHANNELS * THEME_BITS / 8;
    let data_len = byte_rate * THEME_SECONDS;

    let mut wav = Vec::with_capacity(WAV_HEADER_LEN + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk length
    wav.extend_from_slice(&1u16.to_le_bytes()); // 1 = PCM, uncompressed
    wav.extend_from_slice(&THEME_CHANNELS.to_le_bytes());
    wav.extend_from_slice(&THEME_SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&THEME_BITS.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(WAV_HEADER_LEN + data_len as usize, 0);
    wav
}

/// Symbols only a LuaJIT build exports.
///
/// The stock Lua 5.1.4 the game ships exports none of them - they are LuaJIT's own additions
/// to the Lua 5.1 API. Their presence in a file is therefore proof that the file is a LuaJIT,
/// not the engine the game came with. Verified against both artifacts: the game's own
/// `lua51_Win32.dll` exports zero of these, a built LuaJIT exports all five.
const LUAJIT_MARKERS: [&[u8]; 3] = [b"luaJIT_setmode", b"luaJIT_profile_start", b"luaJIT_version"];

/// Whether `bytes` look like a LuaJIT build rather than the game's stock engine.
///
/// A substring search over the whole file rather than a parse of the export table: the export
/// names are in the file either way, and this cannot be broken by a malformed or unexpected PE
/// the way a parser can. The question being asked is "might this be our replacement", and the
/// safe answer when unsure is yes - so a cheap, hard-to-break test is the right one.
fn looks_like_luajit(bytes: &[u8]) -> bool {
    LUAJIT_MARKERS
        .iter()
        .any(|marker| bytes.windows(marker.len()).any(|window| window == *marker))
}

/// [`looks_like_luajit`] for a file that may not be readable.
///
/// An unreadable file answers `false`, so the caller carries on to the copy it was going to
/// make and reports that failure properly, rather than this check turning an I/O problem into
/// a silent refusal that looks like success.
fn file_is_a_replacement(file: ReplacedFile, path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| file.is_a_replacement(&bytes))
}

/// Whether `bytes` are a WAV whose audio is entirely silence.
///
/// The installer's silent theme is the only all-zero WAV that can plausibly be sitting at that
/// path: the game's own theme is 3.6 minutes of music, and music that is digitally silent from
/// the first sample to the last is not something Firaxis shipped. Structural rather than a
/// hash, so it recognises a silent theme this installer wrote at any length.
///
/// A file that is not a WAV at all answers `false` - it is not ours, so it is the game's as far
/// as this question goes, and the caller's own checks decide what to do with it.
fn is_silent_wav(bytes: &[u8]) -> bool {
    if bytes.len() < WAV_HEADER_LEN || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return false;
    }
    bytes[WAV_HEADER_LEN..].iter().all(|byte| *byte == 0)
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

    /// Save the game's copy - but only the first time.
    ///
    /// The guard is the entire point: by the second Deployment the file in the game is the
    /// installer's own replacement, and saving that would destroy the only copy of the
    /// original.
    ///
    /// A missing file in the game is nothing to save and so nothing to report. A player may
    /// have a Steam verify in flight, or an installation that never had the engine where the
    /// installer looks; neither is a reason to stop a Deployment that has not written yet.
    /// # A LuaJIT already in the game is refused
    ///
    /// The "once" guard only asks whether a backup is already held. It cannot ask whether the
    /// file it is about to save *is* the stock engine, and on a machine where an earlier
    /// installer put LuaJIT in place before this store existed, the first backup banks that
    /// LuaJIT as "the original" - permanently, because the guard then refuses to replace it.
    /// Uninstall afterwards "restores" a LuaJIT and reports success for having done so.
    ///
    /// That is not hypothetical; it is what this store held before this check existed. So a
    /// file carrying LuaJIT's own export names is not banked at all. Nothing is written, and a
    /// later Deployment tries again - which succeeds once the player has the stock engine back,
    /// whether from verifying the game's files or from an uninstall with a good backup.
    pub fn back_up_once(&self, file: ReplacedFile, from: &Path) -> Result<(), InstallError> {
        if self.holds(file) || !from.is_file() || file_is_a_replacement(file, from) {
            return Ok(());
        }
        tree::create_dir_all(&self.root)?;
        tree::copy_file(from, &self.path_of(file))
    }

    /// Whether the held backup is unusable because it is a LuaJIT, not a stock engine.
    ///
    /// Separate from [`Self::restore`] so a caller can tell a player their store is bad
    /// *before* an uninstall comes to depend on it.
    pub fn holds_a_replacement(&self, file: ReplacedFile) -> bool {
        self.holds(file) && file_is_a_replacement(file, &self.path_of(file))
    }

    /// Throw away a held backup that turned out to be a LuaJIT, so a later Deployment can bank
    /// the real engine once the player has put it back.
    ///
    /// Separate from taking a backup because it destroys the only thing standing between a
    /// player and a game they cannot restore: a caller has to ask for it in as many words.
    /// Answers whether anything was discarded.
    pub fn discard_replacement(&self, file: ReplacedFile) -> Result<bool, InstallError> {
        if !self.holds_a_replacement(file) {
            return Ok(false);
        }
        tree::remove_file_if_present(&self.path_of(file))?;
        Ok(true)
    }

    /// Put the original back, if one is held.
    ///
    /// A player who cleared the App Data Store between install and uninstall has no original
    /// left to put back. Uninstall says so and carries on removing everything else, because
    /// failing here would leave the Claimed Folders in the game as well as the replacement.
    ///
    /// A store holding a LuaJIT rather than a stock engine reports `NothingToRestore` too:
    /// writing it back would hand the player the very replacement they asked to be rid of, and
    /// call that a successful restore.
    pub fn restore(&self, file: ReplacedFile, to: &Path) -> Result<Restored, InstallError> {
        if !self.holds(file) || self.holds_a_replacement(file) {
            return Ok(Restored::NothingToRestore);
        }
        tree::copy_file(&self.path_of(file), to)?;
        Ok(Restored::FromBackup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The silent theme is a real WAV in the game's own format.
    ///
    /// The game reads it with the same code path as the original; the only difference is that
    /// every sample is zero. If the header were wrong the menu would not fall silent, it would
    /// fail to load a track - a different and worse outcome.
    #[test]
    fn the_silent_theme_is_a_valid_wav_in_the_games_format() {
        let wav = silent_theme();

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");

        let u16_at = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
        let u32_at = |at: usize| {
            u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]])
        };

        assert_eq!(u32_at(16), 16, "PCM fmt chunk length");
        assert_eq!(u16_at(20), 1, "1 = PCM, uncompressed");
        assert_eq!(u16_at(22), 2, "stereo, as the game's own theme is");
        assert_eq!(u32_at(24), 44_100, "44.1 kHz, as the game's own theme is");
        assert_eq!(u16_at(34), 16, "16-bit, as the game's own theme is");

        // The two lengths in the header have to agree with the file, or a reader that trusts
        // either one reads off the end.
        let data_len = u32_at(40) as usize;
        assert_eq!(data_len, wav.len() - WAV_HEADER_LEN);
        assert_eq!(u32_at(4) as usize, wav.len() - 8, "RIFF size counts all but its own header");

        // Byte rate and block align must match the other fields; players' audio drivers do
        // trust these.
        assert_eq!(u32_at(28), 44_100 * 2 * 16 / 8, "byte rate");
        assert_eq!(u16_at(32), 2 * 16 / 8, "block align");

        assert_eq!(data_len, 44_100 * 2 * 16 / 8 * 60, "one minute");
        assert!(wav[WAV_HEADER_LEN..].iter().all(|b| *b == 0), "silence");
    }

    /// The theme's own "is this mine" test, and the boundary either side of it.
    #[test]
    fn a_silent_wav_is_recognised_and_a_real_one_is_not() {
        assert!(ReplacedFile::MenuTheme.is_a_replacement(&silent_theme()));

        // The game's own theme: a real WAV whose samples are not all zero.
        let mut music = silent_theme();
        music[WAV_HEADER_LEN + 1000] = 0x7f;
        assert!(
            !ReplacedFile::MenuTheme.is_a_replacement(&music),
            "a WAV with any audio in it is the game's, not ours"
        );

        // Not a WAV at all is not ours either.
        assert!(!ReplacedFile::MenuTheme.is_a_replacement(b"MZ this is a dll"));
        assert!(!ReplacedFile::MenuTheme.is_a_replacement(b""));

        // The two Replaced Files do not answer each other's question.
        assert!(!ReplacedFile::LuaEngine.is_a_replacement(&silent_theme()));
    }

    /// Each Replaced File has to land where the game keeps it, and only Expansion2's theme is
    /// touched - the base game's and Gods & Kings' copies are different files.
    #[test]
    fn the_menu_theme_is_the_brave_new_world_one() {
        let folders = GameFolders {
            mods: PathBuf::from("/documents/MODS"),
            dlc: PathBuf::from("/game/Assets/DLC"),
            text: PathBuf::from("/documents/Text"),
            game_root: PathBuf::from("/game"),
        };
        let path = ReplacedFile::MenuTheme.path_in(&folders);

        assert!(path.starts_with(&folders.game_root));
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("OpeningMenu_Exp2.wav"));
        let shown = path.display().to_string().replace('\\', "/");
        assert!(
            shown.contains("Assets/DLC/Expansion2/Sounds/Streamed/Music"),
            "unexpected location: {shown}"
        );
        assert!(!shown.contains("DLC/Expansion/"), "Gods & Kings is left alone");
    }

    /// LuaJIT's own export names, as they appear in a real build. Used to make the test
    /// fixtures look like the thing the check has to recognise.
    const LUAJIT_SHAPED: &[u8] = b"\x4d\x5aPE\0\0 ... luaJIT_setmode ... luaJIT_version_2_1";

    /// A game whose engine is *already* LuaJIT must not have it banked as "the original".
    ///
    /// This is the failure that was found in the field: an earlier installer replaced the
    /// engine before this store existed, so the first backup saved a LuaJIT, and the "once"
    /// guard then made that permanent. Uninstall would have handed the player a LuaJIT and
    /// called it a restore.
    #[test]
    fn a_luajit_in_the_game_is_never_banked_as_the_original() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let store = BackupStore::new(dir.path().join("backups"));
        let game_file = dir.path().join("lua51_Win32.dll");

        let Ok(()) = std::fs::write(&game_file, LUAJIT_SHAPED) else {
            unreachable!("a LuaJIT already in the game")
        };
        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the refusal is not an error")
        };
        assert!(
            !store.holds(ReplacedFile::LuaEngine),
            "a LuaJIT must not be banked as the game's engine"
        );

        // And once the player has the stock engine back, the next attempt banks it.
        let Ok(()) = std::fs::write(&game_file, b"stock lua 5.1") else {
            unreachable!("the stock engine restored by a Steam verify")
        };
        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the retry")
        };
        assert!(store.holds(ReplacedFile::LuaEngine), "the stock engine is banked");
    }

    /// A store written before the check existed may already hold a LuaJIT. Restoring it would
    /// reinstall the replacement and report success, so it reports nothing to restore instead.
    #[test]
    fn a_backup_that_is_really_a_luajit_is_not_restored() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let root = dir.path().join("backups");
        let store = BackupStore::new(root.clone());
        let Ok(()) = std::fs::create_dir_all(&root) else {
            unreachable!("the store")
        };
        let Ok(()) = std::fs::write(root.join("lua51_Win32.dll"), LUAJIT_SHAPED) else {
            unreachable!("a bad backup from before the check")
        };

        assert!(store.holds(ReplacedFile::LuaEngine));
        assert!(store.holds_a_replacement(ReplacedFile::LuaEngine));

        let game_file = dir.path().join("lua51_Win32.dll");
        let Ok(()) = std::fs::write(&game_file, b"whatever is there now") else {
            unreachable!("the game file")
        };
        let Ok(outcome) = store.restore(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("the restore")
        };
        assert_eq!(outcome, Restored::NothingToRestore);
        let Ok(after) = std::fs::read(&game_file) else {
            unreachable!("read back")
        };
        assert_eq!(after, b"whatever is there now", "the game file was not touched");
    }

    /// Discarding a bad backup is what lets a player recover: verify the game's files, run the
    /// installer again, and the real engine is banked.
    #[test]
    fn a_bad_backup_can_be_discarded_and_a_good_one_taken() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let root = dir.path().join("backups");
        let store = BackupStore::new(root.clone());
        let Ok(()) = std::fs::create_dir_all(&root) else {
            unreachable!("the store")
        };
        let Ok(()) = std::fs::write(root.join("lua51_Win32.dll"), LUAJIT_SHAPED) else {
            unreachable!("a bad backup")
        };

        let Ok(discarded) = store.discard_replacement(ReplacedFile::LuaEngine) else {
            unreachable!("the discard")
        };
        assert!(discarded);
        assert!(!store.holds(ReplacedFile::LuaEngine));

        // A good backup is never discarded by the same call.
        let game_file = dir.path().join("lua51_Win32.dll");
        let Ok(()) = std::fs::write(&game_file, b"stock lua 5.1") else {
            unreachable!("the stock engine")
        };
        let Ok(()) = store.back_up_once(ReplacedFile::LuaEngine, &game_file) else {
            unreachable!("banking the stock engine")
        };
        let Ok(again) = store.discard_replacement(ReplacedFile::LuaEngine) else {
            unreachable!("the second discard")
        };
        assert!(!again, "a stock engine is not a replacement");
        assert!(store.holds(ReplacedFile::LuaEngine));
    }

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
    /// That is a thing to report, not a failure - uninstall still removes everything else.
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
    /// that point - refusing to continue would be a failure invented by the installer.
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
    /// - the one place the installer had never written before ADR-0006.
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
