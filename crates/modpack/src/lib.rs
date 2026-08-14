//! The Modpack's database engine (ticket 11).
//!
//! The Modpack install mode generates a DLC folder in which the game finds, instead of the
//! mods themselves, two XML dumps of the *merged* databases: every mod's `.sql` and `.xml`
//! update applied, in activation order, on top of the game's own merged vanilla bases
//! (`cache/Civ5DebugDatabase.db` and `cache/Localization-Merged.db`). This crate does exactly
//! that part — copy the bases into scratch, apply the updates, dump both databases — and
//! nothing else; the Core stages every plain file of the Modpack itself.
//!
//! The dump format is not ours to design. The in-game "Modpack Maker for VP" Lua
//! (`ModpackMaker.lua`, azum4roll) already produces dumps the game demonstrably loads, so
//! [`dump`] replicates its output byte for byte — including its quirks, like the blank lines
//! its line-buffered writer leaves behind. See that module for the details.
//!
//! It is a separate crate from the Core for one reason: the Core has no dependencies and must
//! keep having none (rule 1), while this needs a SQLite engine and an XML parser. The Core
//! consumes it through the [`ModpackAssembler`] boundary. Nothing here depends on egui either.

// Rule 9: everything in this crate is reachable from the UI, through the `ModpackAssembler`
// boundary.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unreachable,
    clippy::unimplemented
)]

mod apply;
mod cache;
mod dump;
mod gamedata;

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{
    BoundaryError, CacheState, ModpackAssembler, ModpackDatabaseJob, ProgressReporter,
};
use rusqlite::Connection;

/// The real modpack assembler: rusqlite against copies of the game's own databases.
///
/// Stateless — every call carries its paths in. Construct one and hand it to the Core.
pub struct SqliteModpackAssembler;

impl SqliteModpackAssembler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SqliteModpackAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl ModpackAssembler for SqliteModpackAssembler {
    fn cache_state(&self, gameplay_db: &Path) -> Result<CacheState, BoundaryError> {
        cache::cache_state(gameplay_db)
    }

    fn merge_and_dump(
        &self,
        job: &ModpackDatabaseJob,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        fs::create_dir_all(&job.scratch_dir).map_err(|error| {
            scratch_error(
                format!("create scratch directory {}", job.scratch_dir.display()),
                &error,
            )
        })?;

        let gameplay_copy = job.scratch_dir.join("gameplay.db");
        let text_copy = job.scratch_dir.join("localization.db");
        prepare_scratch_copy(&job.gameplay_base, &gameplay_copy)?;
        prepare_scratch_copy(&job.text_base, &text_copy)?;

        let mut gameplay = open_scratch(&gameplay_copy)?;
        let mut text = open_scratch(&text_copy)?;

        for update in &job.updates {
            apply::apply_update(update, &mut gameplay, &mut text, progress)?;
        }

        dump::dump_gameplay(&gameplay, &job.gameplay_dump)?;
        dump::dump_text(&text, &job.text_dump)?;
        Ok(())
    }
}

/// Rule 10's two halves for a scratch-space failure: a sentence for the user, the raw IO
/// error for the log.
fn scratch_error(action: String, error: &std::io::Error) -> BoundaryError {
    BoundaryError::new(
        "Preparing the Modpack's working databases failed — check free disk space and try again.",
        format!("{action}: {error}"),
    )
}

/// Place a fresh copy of a base database at `dest`, burying any previous failed run.
///
/// The main file is simply overwritten, but a stale rollback journal or WAL next to it would
/// make SQLite "recover" the fresh copy into garbage — those are deleted first.
fn prepare_scratch_copy(base: &Path, dest: &Path) -> Result<(), BoundaryError> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = dest.as_os_str().to_owned();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if let Err(error) = fs::remove_file(&sidecar)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(scratch_error(
                format!("remove leftover {}", sidecar.display()),
                &error,
            ));
        }
    }
    fs::copy(base, dest).map_err(|error| {
        scratch_error(
            format!("copy {} to {}", base.display(), dest.display()),
            &error,
        )
    })?;
    Ok(())
}

/// Open a scratch copy read-write, tuned for a merge whose durability does not matter: the
/// copy is thrown away on any failure, so the journal lives in memory and syncs are off.
fn open_scratch(path: &Path) -> Result<Connection, BoundaryError> {
    let failure = |detail: String| {
        BoundaryError::new(
            "Opening the Modpack's working databases failed — check free disk space and try again.",
            detail,
        )
    };
    let conn = Connection::open(path)
        .map_err(|error| failure(format!("open {}: {error}", path.display())))?;
    conn.pragma_update(None, "journal_mode", "MEMORY")
        .and_then(|()| conn.pragma_update(None, "synchronous", "OFF"))
        .map_err(|error| failure(format!("tune {}: {error}", path.display())))?;
    Ok(conn)
}
