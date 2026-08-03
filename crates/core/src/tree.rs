//! File-tree operations used by Sync. Every destination path here is derived from a
//! Claimed Folder root by the caller (rule 6); nothing in this module invents one.

use std::fs;
use std::path::Path;

use crate::BUILT_DLL_FILE_NAME;
use crate::error::InstallError;

/// Should this entry of the Installation Source be left out of the Deployment?
///
/// The spec's standard exclusions are "project files, source art, docs, checked-in DLLs".
/// The walking skeleton implements the two that would actively break an install:
///
/// * checked-in DLLs — ADR-0001 says the repository's DLL is stale outside release commits
///   and is never deployed; the Built DLL replaces it.
/// * ModBuddy project files — never part of a working mod folder.
///
/// Source art and docs are cosmetic bloat rather than breakage, and their real paths come
/// from the InnoSetup script, which ticket 02 works through.
fn is_excluded(name: &str) -> bool {
    if name.eq_ignore_ascii_case(BUILT_DLL_FILE_NAME) {
        return true;
    }
    let lowercase = name.to_ascii_lowercase();
    lowercase.ends_with(".civ5proj") || lowercase.ends_with(".civ5proj.user")
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

pub(crate) fn create_dir_all(path: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(path).map_err(|cause| InstallError::Deployment {
        action: "create directory",
        path: path.to_path_buf(),
        cause,
    })
}

/// Copy the tree at `from` into `to`, applying [`is_excluded`].
///
/// Entries are visited in sorted order so that two runs over the same source do the same
/// operations in the same sequence (rule 8).
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), InstallError> {
    create_dir_all(to)?;

    let mut entries = Vec::new();
    let dir = fs::read_dir(from).map_err(|cause| InstallError::Deployment {
        action: "read directory",
        path: from.to_path_buf(),
        cause,
    })?;
    for entry in dir {
        let entry = entry.map_err(|cause| InstallError::Deployment {
            action: "read directory entry in",
            path: from.to_path_buf(),
            cause,
        })?;
        entries.push(entry.path());
    }
    entries.sort();

    for source in entries {
        let Some(name) = source.file_name().and_then(|name| name.to_str()) else {
            // A non-UTF-8 name cannot be one of ours; skipping is safer than guessing.
            continue;
        };
        if is_excluded(name) {
            continue;
        }
        let destination = to.join(name);
        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            copy_file(&source, &destination)?;
        }
    }

    Ok(())
}

pub(crate) fn copy_file(from: &Path, to: &Path) -> Result<(), InstallError> {
    fs::copy(from, to).map_err(|cause| InstallError::Deployment {
        action: "copy into",
        path: to.to_path_buf(),
        cause,
    })?;
    Ok(())
}
