//! Is the game's database cache a usable Modpack base?
//!
//! The Modpack build starts from `cache/Civ5DebugDatabase.db`, which the game rewrites on
//! every launch as the merge of whatever was active. Only an unmodded launch leaves the
//! vanilla base+DLC merge this crate needs; a modded launch bakes the mods in, and a Modpack
//! built on top of that would apply everything twice.

use std::path::Path;

use civ5vp_core::{BoundaryError, CacheState};
use rusqlite::{Connection, OpenFlags};

/// The game's record of which mods are installed, enabled and activated, beside the gameplay
/// database in the cache folder.
const MODS_DATABASE_FILE_NAME: &str = "Civ5ModsDatabase.db";

/// Three things have to hold before the file is called pristine (ADR-0011):
///
/// - no `CustomModOptions`, which the Community Patch's own SQL creates;
/// - no mod `Activated` in the game's mod database - `Enabled` says nothing, it is only the tick
///   in the Mods menu, and pristine caches have plenty;
/// - the mod database not written after the gameplay database, or its `Activated` column no
///   longer speaks for that file.
///
/// It must also hold the full vanilla merge - `Civilizations`, populated. A cache with no
/// usable mod database has nothing to vouch for it and is treated like an unreadable one: the
/// game writes one on every launch.
///
/// Both files are only read.
pub(crate) fn cache_state(gameplay_db: &Path) -> Result<CacheState, BoundaryError> {
    let unreadable = |detail: String| {
        BoundaryError::new(
            "The game's database cache is missing or unreadable - start Civilization V once \
             to the main menu, quit it, and try again.",
            detail,
        )
    };

    let conn = Connection::open_with_flags(gameplay_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| unreadable(format!("open {}: {error}", gameplay_db.display())))?;

    // The first actual read of the file - a text file posing as a database fails here.
    let table_exists = |name: &str| -> Result<bool, rusqlite::Error> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    };

    let modded = table_exists("CustomModOptions")
        .map_err(|error| unreadable(format!("read {}: {error}", gameplay_db.display())))?;
    if modded {
        return Ok(CacheState::Modded);
    }

    let has_civilizations = table_exists("Civilizations")
        .map_err(|error| unreadable(format!("read {}: {error}", gameplay_db.display())))?;
    if !has_civilizations {
        return Err(unreadable(format!(
            "{} has no Civilizations table - not a merged game database",
            gameplay_db.display()
        )));
    }

    let civilizations: i64 = conn
        .query_row("SELECT COUNT(*) FROM Civilizations", [], |row| row.get(0))
        .map_err(|error| unreadable(format!("read {}: {error}", gameplay_db.display())))?;
    if civilizations == 0 {
        return Err(unreadable(format!(
            "{} has an empty Civilizations table - not a merged game database",
            gameplay_db.display()
        )));
    }

    if mods_had_a_hand_in(gameplay_db).map_err(unreadable)? {
        return Ok(CacheState::Modded);
    }
    Ok(CacheState::Pristine)
}

/// Does the mod database beside `gameplay_db` say mods were active, or has it moved on since
/// that file was written? The error is the detail for the log.
fn mods_had_a_hand_in(gameplay_db: &Path) -> Result<bool, String> {
    let path = gameplay_db.with_file_name(MODS_DATABASE_FILE_NAME);
    let failed = |error: &dyn std::fmt::Display| format!("read {}: {error}", path.display());

    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| failed(&error))?;
    let activated: i64 = conn
        .query_row("SELECT COUNT(*) FROM Mods WHERE Activated", [], |row| {
            row.get(0)
        })
        .map_err(|error| failed(&error))?;
    if activated > 0 {
        return Ok(true);
    }

    let written = |file: &Path| std::fs::metadata(file).and_then(|meta| meta.modified());
    let mods_written = written(&path).map_err(|error| failed(&error))?;
    let gameplay_written = written(gameplay_db).map_err(|error| failed(&error))?;
    Ok(mods_written > gameplay_written)
}
