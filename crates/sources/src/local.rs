//! The Local Repo: a developer's own checkout, used byte-for-byte as it is.
//!
//! There is deliberately no git in this file. User story 29 wants uncommitted changes
//! installed, which means the working tree is the source — not `HEAD`, not the index. So the
//! installer does not clean it, stash it, check anything out in it, or even read which ref it
//! is on. It reads the path, confirms it is a real directory, and hands it back.
//!
//! The same reasoning picks the `source_identity`: with uncommitted changes in play, no
//! commit can name what will be built, so the identity comes from the working files
//! themselves — [`civ5vp_core::dll_source_identity`] over the DLL's input roots. That is the
//! ticket-07 "for a dirty Local Repo it derives from working-file contents" case, and it
//! reads the tree without writing a byte to it.

use std::path::Path;

use civ5vp_core::MaterializedSource;

use crate::error::{LocalRepoProblem, SourceError};

/// Confirm `path` is somewhere the Core can read a source tree from, and describe it.
pub(crate) fn materialize(path: &Path) -> Result<MaterializedSource, SourceError> {
    let unusable = |problem| SourceError::LocalRepoUnusable {
        path: path.to_path_buf(),
        problem,
    };
    // The Core joins mod-folder names onto whatever comes back. A relative path would send
    // that at the working directory, which is not a place the user chose.
    if !path.is_absolute() {
        return Err(unusable(LocalRepoProblem::NotAbsolute));
    }
    if !path.is_dir() {
        return Err(unusable(LocalRepoProblem::NotADirectory));
    }
    // The one folder every Version of the repository has and nothing else does: the DLL
    // sources. Catching "you picked your Steam library" here, with a sentence, beats letting
    // the Deployment discover a missing mod folder three steps later (ticket 08: "validates
    // the folder is a Community-Patch-DLL checkout").
    if !path.join("CvGameCoreDLL_Expansion2").is_dir() {
        return Err(unusable(LocalRepoProblem::NotACheckout));
    }
    let source_identity = civ5vp_core::dll_source_identity(path).map_err(|unreadable| {
        SourceError::LocalRepoUnusable {
            path: unreadable,
            problem: LocalRepoProblem::UnreadableSourceFile,
        }
    })?;
    Ok(MaterializedSource {
        root: path.to_path_buf(),
        source_identity,
    })
}
