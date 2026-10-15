//! The Local Repo: a developer's own checkout, used byte-for-byte as it is.
//!
//! Uncommitted changes must be installed, which means the working tree is the source - not
//! `HEAD`, not the index. So the installer does not clean it, stash it, or check anything
//! out in it. It reads the path, confirms it is a real directory, and hands it back.
//!
//! The same reasoning picks the `source_identity`: with uncommitted changes in play, no
//! commit can name what will be built, so the identity comes from the working files
//! themselves - [`civ5vp_core::dll_source_identity`] over the DLL's input roots, which reads
//! the tree without writing a byte to it.
//!
//! git is read once, and only read: to name the DLL the way upstream's own build names it
//! ([`git_description`]). A DLL that calls itself "Local" leaves a crash report unable to
//! say which commit crashed, and a `commit_id.inc` left in the checkout by an earlier build
//! is worse - it names a commit, the wrong one.

use std::path::Path;

use civ5vp_core::{MaterializedSource, ProgressReporter, Stage};
use gix::commit::describe::SelectRef;

use crate::error::{LocalRepoProblem, SourceError};

/// Confirm `path` is somewhere the Core can read a source tree from, and describe it.
pub(crate) fn materialize(
    path: &Path,
    progress: &ProgressReporter,
) -> Result<MaterializedSource, SourceError> {
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
    // the Deployment discover a missing mod folder three steps later.
    if !path.join("CvGameCoreDLL_Expansion2").is_dir() {
        return Err(unusable(LocalRepoProblem::NotACheckout));
    }
    let source_identity = civ5vp_core::dll_source_identity(path).map_err(|unreadable| {
        SourceError::LocalRepoUnusable {
            path: unreadable,
            problem: LocalRepoProblem::UnreadableSourceFile,
        }
    })?;
    // Not a reason to stop: a tree git cannot name still builds, under the name "Local".
    let git_description = match git_description(path) {
        Ok(description) => {
            progress.report(Stage::Fetch, format!("Your checkout is at {description}."));
            Some(description)
        }
        Err(problem) => {
            progress.report(
                Stage::Fetch,
                format!(
                    "Git could not say which commit this checkout is on, so the DLL will call \
                     itself \"Local\". ({problem})"
                ),
            );
            None
        }
    };
    Ok(MaterializedSource {
        root: path.to_path_buf(),
        source_identity,
        git_description,
    })
}

/// Name the checkout the way upstream's `update_commit_id` scripts name a DLL built from it.
///
/// `git describe --tags --long` - the nearest tag, the commits since it, and the commit - or
/// `No-Tag <commit>` where no tag is reachable; then `Clean`, or `Dirty` when a tracked file
/// differs from `HEAD`. Untracked files do not count, as upstream's scripts ignore them too.
///
/// Read-only: gix reads `HEAD`, the tags, the index and the tracked files, and writes nothing
/// back - not even the refreshed index that `git status` would.
pub(crate) fn git_description(path: &Path) -> Result<String, String> {
    let repo = gix::open(path).map_err(|error| error.to_string())?;
    let head = repo.head_commit().map_err(|error| error.to_string())?;
    let described = head
        .describe()
        .names(SelectRef::AllTags)
        .try_format()
        .map_err(|error| error.to_string())?;
    let name = match described {
        Some(mut described) => {
            described.long = true;
            described.to_string()
        }
        None => format!(
            "No-Tag {}",
            head.id().shorten().map_err(|error| error.to_string())?
        ),
    };
    let state = if repo.is_dirty().map_err(|error| error.to_string())? {
        "Dirty"
    } else {
        "Clean"
    };
    Ok(format!("{name} {state}"))
}
