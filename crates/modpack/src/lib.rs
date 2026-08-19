//! The Modpack's database engine.
//!
//! The Modpack install mode generates a DLC folder in which the game finds, instead of the
//! mods themselves, two XML dumps of the *merged* databases: every mod's `.sql` and `.xml`
//! update applied, in activation order, on top of the game's own merged vanilla bases
//! (`cache/Civ5DebugDatabase.db` and `cache/Localization-Merged.db`). This crate does exactly
//! that part - copy the bases into scratch, apply the updates, dump both databases - and
//! nothing else; the Core stages every plain file of the Modpack itself.
//!
//! The dump format is not ours to design. The in-game "Modpack Maker for VP" Lua
//! (`ModpackMaker.lua`, azum4roll) already produces dumps the game demonstrably loads, so
//! [`dump`] replicates its output byte for byte - including its quirks, like the blank lines
//! its line-buffered writer leaves behind. See that module for the details.
//!
//! It is a separate crate from the Core for one reason: the Core has no dependencies and must
//! keep having none, while this needs a SQLite engine and an XML parser. The Core consumes it
//! through the [`ModpackAssembler`] boundary. Nothing here depends on egui either.

// Everything in this crate is reachable from the UI, so nothing may panic.
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
/// Stateless - every call carries its paths in. Construct one and hand it to the Core.
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

        // The game runs mod SQL against a gameplay connection with the localization
        // database attached, and Vox Populi leans on it: whole .sql files write
        // `Language_*` tables directly (CoreDiplomacyResponseTextChanges.sql and friends).
        // An unqualified table name that is not in `main` resolves to the attached
        // database, so those writes land in the localization copy - while an unqualified
        // CREATE TABLE still lands in `main`, exactly as it does in the game.
        attach_text(&gameplay, &text_copy)?;

        for update in &job.updates {
            apply::apply_update(update, &mut gameplay, &mut text, progress)?;
        }

        dump::dump_gameplay(&gameplay, &job.gameplay_dump)?;
        dump::dump_text(&text, &job.text_dump)?;
        Ok(())
    }
}

/// Attach the localization scratch copy to the gameplay connection, under the name the
/// merge uses nowhere else - mods address the tables unqualified, never the schema.
fn attach_text(gameplay: &Connection, text_copy: &Path) -> Result<(), BoundaryError> {
    gameplay
        .execute(
            "ATTACH DATABASE ?1 AS localization",
            [text_copy.to_string_lossy().as_ref()],
        )
        .map(|_| ())
        .map_err(|error| {
            BoundaryError::new(
                "Opening the Modpack's working databases failed - check free disk space \
                 and try again.",
                format!("attach {}: {error}", text_copy.display()),
            )
        })
}

fn scratch_error(action: String, error: &std::io::Error) -> BoundaryError {
    BoundaryError::new(
        "Preparing the Modpack's working databases failed - check free disk space and try again.",
        format!("{action}: {error}"),
    )
}

/// Place a fresh copy of a base database at `dest`, burying any previous failed run.
///
/// The main file is simply overwritten, but a stale rollback journal or WAL next to it would
/// make SQLite "recover" the fresh copy into garbage - those are deleted first.
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
            "Opening the Modpack's working databases failed - check free disk space and try again.",
            detail,
        )
    };
    let conn = Connection::open(path)
        .map_err(|error| failure(format!("open {}: {error}", path.display())))?;
    conn.pragma_update(None, "journal_mode", "MEMORY")
        .and_then(|()| conn.pragma_update(None, "synchronous", "OFF"))
        // Match the game's 2010-era SQLite, which the Community Patch's SQL is written
        // against. Its tables carry *dangling* `REFERENCES Language_en_US` clauses by
        // design - text lives in the other database - and the game neither enforces
        // foreign keys nor re-validates the schema on ALTER TABLE RENAME. The bundled
        // modern SQLite does both by default (rusqlite builds with foreign keys ON),
        // and either one aborts VP's FixTypeConstraints.sql rebuild pattern
        // (CREATE _FIX / INSERT / DROP / RENAME) with "no such table: Language_en_US".
        .and_then(|()| conn.pragma_update(None, "foreign_keys", "OFF"))
        .and_then(|()| conn.pragma_update(None, "legacy_alter_table", "ON"))
        .map_err(|error| failure(format!("tune {}: {error}", path.display())))?;
    Ok(conn)
}
