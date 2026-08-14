//! File-tree operations used by Sync. Every destination path here is derived from a
//! Claimed Folder root by the caller (rule 6); nothing in this module invents one.

use std::fs;
use std::path::Path;

use crate::BUILT_DLL_FILE_NAME;
use crate::error::InstallError;
use crate::plan::SourceSelection;

/// ModBuddy project and solution files. Never part of a working mod folder, and every one of
/// them appears in the official installer's `Excludes:` clauses.
const EXCLUDED_EXTENSIONS: &[&str] = &["civ5proj", "civ5proj.user", "civ5sln", "civ5suo", "xcf"];

/// Developer-facing text files that sit inside the mod folders in the repository.
///
/// The official installer never ships these, though it gets there a different way: it copies
/// from a `Build/` staging tree containing only the files listed in each mod's `.civ5proj`,
/// so anything unlisted is invisible to it. We copy from the repository itself and so need
/// the names. Reading the `.civ5proj` allowlist is the faithful version and belongs with
/// ticket 06, which parses that file anyway to find the DLL's sources; until then this
/// denylist covers the documents that are actually there.
const EXCLUDED_NAMES: &[&str] = &[
    "MANUAL INSTALL.txt",
    "INSTRUCTIONS.txt",
    "Credits.txt",
    "Promotion Icons for VP.txt",
    "SampleContracts.xml",
    "SampleEvents.xml",
];

/// Should this entry of the Installation Source be left out of every Deployment?
///
/// These are the spec's "standard exclusions" — project files, source art, docs, checked-in
/// DLLs — and they apply at every depth, unlike a [`SourceSelection`], which is one
/// configuration's choice about one folder's top level.
///
/// The checked-in DLL is the load-bearing one: ADR-0001 says the repository's DLL is stale
/// outside release commits and is never deployed. The Built DLL replaces it.
pub(crate) fn is_excluded(name: &str) -> bool {
    if name.eq_ignore_ascii_case(BUILT_DLL_FILE_NAME) {
        return true;
    }
    if EXCLUDED_NAMES
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
    {
        return true;
    }
    let lowercase = name.to_ascii_lowercase();
    EXCLUDED_EXTENSIONS
        .iter()
        .any(|extension| lowercase.ends_with(&format!(".{extension}")))
}

/// Copy the part of `from` that `selection` admits into `to`.
///
/// The selection applies to the top level only — nested folders are copied whole. That is the
/// official installer's meaning too: its `\LUA` exclusion is anchored to the source root, so a
/// `LUA` folder deeper in the tree survives.
pub(crate) fn copy_selected(
    from: &Path,
    to: &Path,
    selection: &SourceSelection,
) -> Result<(), InstallError> {
    create_dir_all(to)?;

    for source in sorted_entries(from)? {
        let Some(name) = source.file_name().and_then(|name| name.to_str()) else {
            // A non-UTF-8 name cannot be one of ours; skipping is safer than guessing.
            continue;
        };
        if is_excluded(name) || !selection.admits(name) {
            continue;
        }
        let destination = to.join(name);
        if source.is_dir() {
            // Below the top level the selection no longer applies, only the standard
            // exclusions — which is what anchors an entry like `LUA` to the source root.
            copy_selected(&source, &destination, &SourceSelection::Everything)?;
        } else {
            copy_file(&source, &destination)?;
        }
    }

    Ok(())
}

/// Copy `from` into `to` verbatim — no selection, no standard exclusions.
///
/// For trees the Core assembled itself (the Modpack stage, ticket 11): the exclusions exist
/// to keep repository clutter out of the game, and everything in an assembled tree is there
/// because the Core put it there — including the Built DLL, which `is_excluded` would strip.
pub(crate) fn copy_all(from: &Path, to: &Path) -> Result<(), InstallError> {
    create_dir_all(to)?;
    for source in sorted_entries(from)? {
        let Some(name) = source.file_name() else {
            continue;
        };
        let destination = to.join(name);
        if source.is_dir() {
            copy_all(&source, &destination)?;
        } else {
            copy_file(&source, &destination)?;
        }
    }
    Ok(())
}

/// Does this source folder hold anything `selection` would take?
///
/// A named-entries selection matching nothing means the source is not shaped the way this
/// installer expects — a file renamed upstream, most likely. Deploying the empty folder that
/// would result looks like success and leaves the player with a mod that silently does
/// nothing, so it is reported instead. Asked before the Deployment starts, never during it:
/// rule 7 wants the game untouched when anything is wrong.
pub(crate) fn holds_anything_selected(
    from: &Path,
    selection: &SourceSelection,
) -> Result<bool, InstallError> {
    for source in sorted_entries(from)? {
        let Some(name) = source.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_excluded(name) && selection.admits(name) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Delete `path` and everything under it, if it exists at all.
pub(crate) fn remove_if_present(path: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(InstallError::Deployment {
            action: "remove",
            path: path.to_path_buf(),
            cause,
        }),
    }
}

/// Delete a single file, if it is there at all.
pub(crate) fn remove_file_if_present(path: &Path) -> Result<(), InstallError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(InstallError::Deployment {
            action: "remove",
            path: path.to_path_buf(),
            cause,
        }),
    }
}

/// Empty a directory without removing the directory itself, if it is there at all.
///
/// This exists for the game's `cache` folder and nothing else. The folder is kept because the
/// game expects to find it; its contents go because a stale cache is the classic cause of a
/// corrupt-looking install (user story 23).
pub(crate) fn clear_directory_contents(path: &Path) -> Result<(), InstallError> {
    let dir = match fs::read_dir(path) {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(cause) => {
            return Err(InstallError::Deployment {
                action: "read directory",
                path: path.to_path_buf(),
                cause,
            });
        }
    };

    let mut entries = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|cause| InstallError::Deployment {
            action: "read directory entry in",
            path: path.to_path_buf(),
            cause,
        })?;
        entries.push(entry.path());
    }
    entries.sort();

    for entry in entries {
        if entry.is_dir() {
            remove_if_present(&entry)?;
        } else {
            remove_file_if_present(&entry)?;
        }
    }

    Ok(())
}

pub(crate) fn create_dir_all(path: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(path).map_err(|cause| InstallError::Deployment {
        action: "create directory",
        path: path.to_path_buf(),
        cause,
    })
}

/// The entries of `dir`, sorted, so that two runs over the same source do the same operations
/// in the same sequence (rule 8).
fn sorted_entries(dir: &Path) -> Result<Vec<std::path::PathBuf>, InstallError> {
    let read = fs::read_dir(dir).map_err(|cause| InstallError::Deployment {
        action: "read directory",
        path: dir.to_path_buf(),
        cause,
    })?;

    let mut entries = Vec::new();
    for entry in read {
        let entry = entry.map_err(|cause| InstallError::Deployment {
            action: "read directory entry in",
            path: dir.to_path_buf(),
            cause,
        })?;
        entries.push(entry.path());
    }
    entries.sort();
    Ok(entries)
}

pub(crate) fn copy_file(from: &Path, to: &Path) -> Result<(), InstallError> {
    fs::copy(from, to).map_err(|cause| InstallError::Deployment {
        action: "copy into",
        path: to.to_path_buf(),
        cause,
    })?;
    Ok(())
}
