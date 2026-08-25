//! Which ref a developer's own checkout is sitting on.
//!
//! Read for the screen and nothing else. It is deliberately *not* the Local Repo's identity:
//! Dev mode installs the working tree, uncommitted changes and all, so no ref can describe
//! what will actually be built - that is what [`crate::dll_source_identity`] is for, and it
//! hashes the files themselves. This answers a smaller question, "which branch am I on",
//! which is worth showing precisely because the folder path does not say.
//!
//! No git library and no `git` process: `HEAD` is one short file whose format has not moved
//! in fifteen years, and reading it cannot disturb a working tree the way anything heavier
//! might.

use std::path::{Path, PathBuf};

/// What `HEAD` points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutRef {
    /// The ordinary case: `HEAD` names a branch.
    Branch(String),
    /// `HEAD` holds a commit directly - mid-rebase, mid-bisect, or a deliberate
    /// `git checkout <sha>`. Worth distinguishing on screen, because "no branch" surprises
    /// people who think they are on one.
    Detached(String),
}

impl CheckoutRef {
    /// How to say it on one line.
    pub fn describe(&self) -> String {
        match self {
            Self::Branch(name) => format!("Branch {name}"),
            Self::Detached(commit) => format!("Not on a branch - detached at {commit}"),
        }
    }
}

/// The ref `repo` is checked out at, or `None` if this is not a git checkout at all.
///
/// `None` is an ordinary answer, not an error: a source tree unpacked from an archive is a
/// perfectly good Local Repo and simply has nothing to report.
pub fn current_ref(repo: &Path) -> Option<CheckoutRef> {
    let head = std::fs::read_to_string(git_dir(repo)?.join("HEAD")).ok()?;
    let head = head.trim();
    // `ref: refs/heads/some/branch` - branch names may contain slashes, so this takes the
    // whole remainder rather than the last segment.
    if let Some(reference) = head.strip_prefix("ref: ") {
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        return (!name.is_empty()).then(|| CheckoutRef::Branch(name.to_owned()));
    }
    // Otherwise HEAD is a raw commit id. Shortened for the screen, and checked for shape so
    // that a corrupt or unexpected HEAD reports nothing rather than something misleading.
    let is_commit = head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit());
    is_commit.then(|| CheckoutRef::Detached(head.chars().take(7).collect()))
}

/// The directory holding `HEAD`.
///
/// Usually `<repo>/.git`. In a linked worktree or a submodule, `.git` is a *file* holding
/// `gitdir: <path>` instead - which is not exotic here, since this project's own fork work is
/// done in a worktree.
fn git_dir(repo: &Path) -> Option<PathBuf> {
    let dot_git = repo.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = std::fs::read_to_string(&dot_git).ok()?;
    let target = Path::new(pointer.trim().strip_prefix("gitdir: ")?);
    Some(if target.is_absolute() {
        target.to_path_buf()
    } else {
        repo.join(target)
    })
}
