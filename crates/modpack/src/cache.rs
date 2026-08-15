//! Is the game's database cache a usable Modpack base?
//!
//! The Modpack build starts from `cache/Civ5DebugDatabase.db`, which the game rewrites on
//! every launch as the merge of whatever was active. Only an unmodded launch leaves the
//! vanilla base+DLC merge this crate needs; a modded launch bakes the mods in, and a Modpack
//! built on top of that would apply everything twice.

use std::path::Path;

use civ5vp_core::{BoundaryError, CacheState};
use rusqlite::{Connection, OpenFlags};

/// The tell the two states are told apart by: `CustomModOptions` is created by the Community
/// Patch's own SQL, so its presence means a modded session wrote the cache. A pristine cache
/// lacks it but still holds the full vanilla merge — `Civilizations`, populated.
pub(crate) fn cache_state(gameplay_db: &Path) -> Result<CacheState, BoundaryError> {
    let unreadable = |detail: String| {
        BoundaryError::new(
            "The game's database cache is missing or unreadable — start Civilization V once \
             to the main menu, quit it, and try again.",
            detail,
        )
    };

    let conn = Connection::open_with_flags(gameplay_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| unreadable(format!("open {}: {error}", gameplay_db.display())))?;

    // The first actual read of the file — a text file posing as a database fails here.
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
            "{} has no Civilizations table — not a merged game database",
            gameplay_db.display()
        )));
    }

    let civilizations: i64 = conn
        .query_row("SELECT COUNT(*) FROM Civilizations", [], |row| row.get(0))
        .map_err(|error| unreadable(format!("read {}: {error}", gameplay_db.display())))?;
    if civilizations == 0 {
        return Err(unreadable(format!(
            "{} has an empty Civilizations table — not a merged game database",
            gameplay_db.display()
        )));
    }

    Ok(CacheState::Pristine)
}
