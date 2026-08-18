//! The pinned LuaJIT checkout: one commit, fetched once, reused forever.
//!
//! Pinned by commit rather than by tag or tarball — see `docs/pinned-artifacts.md`. Tags move,
//! and GitHub's generated archives are not byte-stable, so the commit SHA is the only
//! self-verifying identity on offer.
//!
//! ## Shape on disk
//!
//! ```text
//! <App Data Store>/luajit-cache/
//!     .luajit-commit               the commit the tree beside it was checked out from
//!     LuaJIT/                      the working tree: src/, dynasm/, and the rest
//!     LuaJIT/.git/                 the managed repository, one commit deep
//! ```
//!
//! The stamp file lives *outside* the working tree on purpose: it is written last, after every
//! file is on disk, so a checkout interrupted half way cannot be mistaken for a finished one,
//! and nothing the build reads can collide with it. It is also what makes a second Deployment
//! need no network at all.
//!
//! Unlike the Upstream Cache this holds exactly one commit forever, so there is no
//! incrementality to preserve: a retry after a failed attempt starts from an empty directory
//! rather than trying to salvage a half-fetched object store.

use std::fs;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use civ5vp_core::{BoundaryError, ProgressReporter, Stage};
use gix::remote::Direction;
use gix::remote::fetch::{Shallow, Tags};

use crate::error::chain;

/// Upstream LuaJIT. The only LuaJIT URL in the crate.
pub const LUAJIT_URL: &str = "https://github.com/LuaJIT/LuaJIT.git";

/// The pinned commit on branch `v2.1` (`docs/pinned-artifacts.md`).
pub const LUAJIT_COMMIT: &str = "1edc3e52b67eaf6ce5f809be8e17d6862594b8bc";

/// Records which commit the tree beside it was checked out from.
const STAMP_FILE_NAME: &str = ".luajit-commit";

/// The directory the checkout goes into, inside the cache root.
const CHECKOUT_DIR_NAME: &str = "LuaJIT";

/// Written inside the checkout's own `.git` when this code creates it, and checked before
/// anything is deleted. Its only job is to make "is this directory ours?" answerable, because
/// [`LuaJitCache::new`] accepts any path and a caller that passed a wrong one would otherwise
/// have it removed.
const CACHE_MARKER: &str = "civ5vp-luajit-cache";

const CACHE_MARKER_CONTENTS: &str = "This folder is the Civ 5 VP Installer's LuaJIT source cache. It is safe to delete when \
     the installer is not running; the installer will fetch it again.\n";

/// One commit and nothing behind it. LuaJIT's history is a decade deep and the build reads
/// only the tree.
const SHALLOW_DEPTH: NonZeroU32 = NonZeroU32::MIN;

/// Nothing cancels a fetch yet.
static NEVER_INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The LuaJIT checkout inside the App Data Store.
pub struct LuaJitCache {
    root: PathBuf,
}

impl LuaJitCache {
    /// `root` is the cache directory inside the App Data Store, created on first use.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Where the built source tree lives: the directory holding `src/` and `dynasm/`.
    pub fn source_root(&self) -> PathBuf {
        self.root.join(CHECKOUT_DIR_NAME)
    }

    pub(crate) fn stamp_path(&self) -> PathBuf {
        self.root.join(STAMP_FILE_NAME)
    }

    /// Whether the checkout on disk is the pinned commit, with the two directories the build
    /// reads actually present.
    ///
    /// Both halves are checked because they can disagree: a stamp with no tree is what a
    /// half-deleted cache looks like, and trusting it would send the build at a directory that
    /// is not there.
    pub(crate) fn already_has_the_pinned_commit(&self) -> bool {
        let source = self.source_root();
        source.join("src").is_dir()
            && source.join("dynasm").is_dir()
            && fs::read_to_string(self.stamp_path())
                .is_ok_and(|stamp| stamp.trim() == LUAJIT_COMMIT)
    }

    /// Fetch the pinned commit if it is not already here, and return the source root.
    pub fn materialize(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError> {
        if self.already_has_the_pinned_commit() {
            progress.report(Stage::Fetch, "LuaJIT source is already here.");
            return Ok(self.source_root());
        }
        progress.report(Stage::Fetch, "Fetching the LuaJIT source.");
        self.fetch_pinned_commit(progress)?;
        // Written last: until this line the tree on disk is not to be trusted, and a run
        // interrupted before it simply fetches again.
        fs::write(self.stamp_path(), LUAJIT_COMMIT).map_err(|error| {
            BoundaryError::new(
                "The installer could not record which LuaJIT version it fetched.",
                format!("could not write {}: {error}", self.stamp_path().display()),
            )
        })?;
        progress.report(Stage::Fetch, "LuaJIT source is ready.");
        Ok(self.source_root())
    }

    /// Fetch [`LUAJIT_COMMIT`] one commit deep and write its tree into [`Self::source_root`].
    ///
    /// The shape is the Upstream Cache's, for the same reasons (ADR-0004): a depth-1 fetch of
    /// exactly one ref, no tags, and a checkout into a directory that was emptied first. The
    /// one difference is what is asked for — a commit id rather than a ref chosen at runtime,
    /// which is the same thing the Upstream Cache does for an Arbitrary Ref or an unofficial
    /// build, and which GitHub serves because the commit is reachable.
    fn fetch_pinned_commit(&self, progress: &ProgressReporter) -> Result<(), BoundaryError> {
        let source = self.source_root();
        // A directory left over from an attempt that failed part way. There is no object store
        // worth salvaging in a one-commit cache, so it goes — but only once it has proved it is
        // ours to remove.
        if source.exists() {
            self.remove_previous_attempt()?;
        }
        let repo = self.init(&source)?;

        let local_ref = format!("refs/civ5vp/luajit/{LUAJIT_COMMIT}");
        let refspec = format!("+{LUAJIT_COMMIT}:{local_ref}");
        let remote = repo
            .remote_at(LUAJIT_URL)
            .map_err(|error| self.unreachable(&error))?
            .with_refspecs([refspec.as_str()], Direction::Fetch)
            .map_err(|error| self.unreachable(&error))?
            // LuaJIT carries a tag per release; none of them is wanted, and each would drag in
            // a snapshot of its own.
            .with_fetch_tags(Tags::None);

        let connection = remote
            .connect(Direction::Fetch)
            .map_err(|error| self.unreachable(&error))?;
        let prepared = connection
            .prepare_fetch(
                gix::progress::Discard,
                gix::remote::ref_map::Options::default(),
            )
            .map_err(|error| self.unreachable(&error))?;
        prepared
            .with_shallow(Shallow::DepthAtRemote(SHALLOW_DEPTH))
            .receive(gix::progress::Discard, &NEVER_INTERRUPTED)
            .map_err(|error| self.unreachable(&error))?;

        // Asked for by id, so what came back is the pin or the fetch failed: there is no ref
        // name in between that could have moved under us.
        let commit = gix::ObjectId::from_hex(LUAJIT_COMMIT.as_bytes())
            .map_err(|error| self.checkout_failed(chain(&error)))?;
        self.checkout(&repo, commit, progress)
    }

    /// Create the cache directory and the repository inside it, marker first.
    fn init(&self, source: &std::path::Path) -> Result<gix::Repository, BoundaryError> {
        fs::create_dir_all(source).map_err(|error| self.unusable(error.to_string()))?;
        let repository = gix::init(source).map_err(|error| self.unusable(chain(&error)))?;
        fs::write(
            repository.git_dir().join(CACHE_MARKER),
            CACHE_MARKER_CONTENTS,
        )
        .map_err(|error| self.unusable(error.to_string()))?;
        Ok(repository)
    }

    /// Remove the leftovers of an earlier attempt — refusing if the directory is not one this
    /// code created, since `new` accepts any path at all.
    fn remove_previous_attempt(&self) -> Result<(), BoundaryError> {
        let source = self.source_root();
        if !source.join(".git").join(CACHE_MARKER).is_file() {
            return Err(BoundaryError::new(
                format!(
                    "There is already a folder at {} that the installer did not create, so it \
                     was left alone. Move or delete it and try again.",
                    source.display()
                ),
                format!("{CACHE_MARKER} missing under {}", source.display()),
            ));
        }
        fs::remove_dir_all(&source).map_err(|error| {
            self.unusable(format!("could not remove {}: {error}", source.display()))
        })
    }

    /// Write `commit`'s tree into the checkout directory.
    fn checkout(
        &self,
        repo: &gix::Repository,
        commit: gix::ObjectId,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        progress.report(Stage::Fetch, "Unpacking the LuaJIT source.");
        let failed = |detail: String| self.checkout_failed(detail);
        let tree = repo
            .find_object(commit)
            .map_err(|error| failed(chain(&error)))?
            .peel_to_tree()
            .map_err(|error| failed(chain(&error)))?
            .id;
        let mut index = repo
            .index_from_tree(&tree)
            .map_err(|error| failed(chain(&error)))?;
        let objects = repo
            .objects
            .clone()
            .into_arc()
            .map_err(|error| failed(chain(&error)))?;

        let options = gix::worktree::state::checkout::Options {
            // True because `fetch_pinned_commit` starts from a directory holding nothing but
            // the `.git` it just made.
            destination_is_initially_empty: true,
            // No filters and no attribute sources: blobs are written exactly as stored, and no
            // `.gitattributes` in the repository may cause an external filter program to run.
            ..Default::default()
        };
        gix::worktree::state::checkout(
            &mut index,
            self.source_root(),
            objects,
            &gix::progress::Discard,
            &gix::progress::Discard,
            &NEVER_INTERRUPTED,
            options,
        )
        .map_err(|error| failed(chain(&error)))?;
        index
            .write(gix::index::write::Options::default())
            .map_err(|error| failed(chain(&error)))?;
        Ok(())
    }

    fn unreachable(&self, error: &dyn std::error::Error) -> BoundaryError {
        BoundaryError::new(
            "Could not download the LuaJIT source. Check your internet connection and try \
             again — nothing has been changed. You can also turn the LuaJIT option off.",
            format!(
                "fetch of {LUAJIT_COMMIT} from {LUAJIT_URL} failed: {}",
                chain(error)
            ),
        )
    }

    fn unusable(&self, detail: String) -> BoundaryError {
        BoundaryError::new(
            format!(
                "The installer could not use its LuaJIT download folder at {}. Check that the \
                 folder is not read-only.",
                self.root.display()
            ),
            format!("luajit cache at {} unusable: {detail}", self.root.display()),
        )
    }

    fn checkout_failed(&self, detail: String) -> BoundaryError {
        BoundaryError::new(
            "The LuaJIT source was downloaded but could not be unpacked. Check that you have \
             free disk space and try again.",
            format!("checkout of {LUAJIT_COMMIT} failed: {detail}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pin is a commit, never a tag: tags move, and GitHub's generated tarballs are not
    /// byte-stable, so the commit SHA is the only self-verifying identity available.
    #[test]
    fn the_pin_is_a_full_commit_sha() {
        assert_eq!(LUAJIT_COMMIT.len(), 40);
        assert!(LUAJIT_COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(LUAJIT_URL.ends_with("LuaJIT.git"));
    }

    /// A cache that already holds the pinned commit is reused rather than refetched — the same
    /// rule the Upstream Cache follows, and what keeps a second Deployment offline.
    #[test]
    fn an_existing_checkout_is_reused() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let cache = LuaJitCache::new(dir.path().join("luajit-cache"));
        let Ok(()) = fs::create_dir_all(cache.source_root().join("src")) else {
            unreachable!("src")
        };
        let Ok(()) = fs::create_dir_all(cache.source_root().join("dynasm")) else {
            unreachable!("dynasm")
        };
        let Ok(()) = fs::write(cache.stamp_path(), LUAJIT_COMMIT) else {
            unreachable!("stamp")
        };
        assert!(cache.already_has_the_pinned_commit());
    }

    /// A cache stamped with some other commit is not the pin. This is what makes changing
    /// [`LUAJIT_COMMIT`] in a later release actually refetch, instead of building whatever
    /// happened to be left on the player's disk.
    #[test]
    fn a_checkout_of_another_commit_is_not_the_pin() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let cache = LuaJitCache::new(dir.path().join("luajit-cache"));
        let Ok(()) = fs::create_dir_all(cache.source_root().join("src")) else {
            unreachable!("src")
        };
        let Ok(()) = fs::create_dir_all(cache.source_root().join("dynasm")) else {
            unreachable!("dynasm")
        };
        let Ok(()) = fs::write(cache.stamp_path(), "0".repeat(40)) else {
            unreachable!("stamp")
        };
        assert!(!cache.already_has_the_pinned_commit());
    }

    /// The stamp alone is not enough. A player who deleted the tree but left the stamp — or a
    /// run interrupted between the two — must fetch again rather than hand the build a
    /// directory that is not there.
    #[test]
    fn a_stamp_without_a_tree_is_not_the_pin() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let root = dir.path().join("luajit-cache");
        let Ok(()) = fs::create_dir_all(&root) else {
            unreachable!("the cache root")
        };
        let cache = LuaJitCache::new(root);
        let Ok(()) = fs::write(cache.stamp_path(), LUAJIT_COMMIT) else {
            unreachable!("stamp")
        };
        assert!(!cache.already_has_the_pinned_commit());
    }

    /// A directory the installer did not create is never deleted, however wrong it looks —
    /// `new` accepts any path, and a caller that passed the wrong one must not lose it.
    #[test]
    fn a_foreign_directory_is_refused_rather_than_emptied() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let cache = LuaJitCache::new(dir.path().join("luajit-cache"));
        let Ok(()) = fs::create_dir_all(cache.source_root()) else {
            unreachable!("the source root")
        };
        let precious = cache.source_root().join("precious.txt");
        let Ok(()) = fs::write(&precious, "not ours") else {
            unreachable!("write")
        };

        let Err(error) = cache.materialize(&ProgressReporter::silent()) else {
            unreachable!("a directory that is not ours must be refused")
        };
        assert!(error.detail().contains(CACHE_MARKER), "{error:?}");
        assert!(precious.is_file(), "the file must still be there");
    }

    /// The real thing, over the real network: the pinned commit arrives and the build's two
    /// directories are on disk. Ignored by default — every other test here is offline.
    #[test]
    #[ignore = "clones the real LuaJIT repository"]
    fn the_pinned_commit_is_fetched_and_then_reused() {
        let Ok(dir) = tempfile::tempdir() else {
            unreachable!("a temp dir")
        };
        let cache = LuaJitCache::new(dir.path().join("luajit-cache"));

        let Ok(source) = cache.materialize(&ProgressReporter::silent()) else {
            unreachable!("the pinned commit is fetchable")
        };
        assert!(source.join("src/lj_api.c").is_file(), "the library sources");
        assert!(source.join("dynasm/dynasm.lua").is_file(), "the assembler");
        assert!(cache.already_has_the_pinned_commit());

        // The second call must not touch the network: the stamp is the whole point.
        let Ok(again) = cache.materialize(&ProgressReporter::silent()) else {
            unreachable!("a cached checkout is reused")
        };
        assert_eq!(again, source);
    }
}
