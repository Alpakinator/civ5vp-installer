//! The Upstream Cache: the installer-managed clone of `LoneGazebo/Community-Patch-DLL`.
//!
//! ## Why it is shallow rather than blobless
//!
//! The spec's preferred strategy was a blobless partial clone (`--filter=blob:none`). `gix`
//! cannot do one: it never sends a `filter` line, and it has no promisor support, so a
//! partial clone would leave a repository whose blobs can never be filled in. ADR-0004
//! records the measurements behind picking a **depth-1 shallow fetch per Version** instead,
//! which turns out to transfer *less* than a blobless clone would.
//!
//! ## Shape on disk
//!
//! ```text
//! <App Data Store>/upstream-cache/
//!     .git/                        the managed repository — objects for every Version ever fetched
//!     .git/civ5vp-materialized     the commit the tree beside it was written from
//!     (1) Community Patch/…        the working tree of the selected Version
//! ```
//!
//! Objects accumulate and are never pruned. That is the whole incrementality story: the local
//! refs under `refs/civ5vp/` are what the next fetch offers the server as "already have", so
//! content that is the same between two Versions is not sent twice.

use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use civ5vp_core::{ProgressReporter, Stage, Version};
use gix::remote::Direction;
use gix::remote::fetch::{Shallow, Tags};

use crate::error::{SourceError, chain};
use crate::version::{RefTarget, VersionCatalog};

/// The one network source the installer has for mod files (`docs/pinned-artifacts.md` §6).
pub const UPSTREAM_URL: &str = "https://github.com/LoneGazebo/Community-Patch-DLL.git";

/// Records which commit the working tree beside it was written from. Kept inside `.git` so it
/// never shows up in the tree the Core walks.
const MATERIALIZED_MARKER: &str = "civ5vp-materialized";

/// One commit per Version and nothing behind it — the whole point of the strategy.
const SHALLOW_DEPTH: NonZeroU32 = NonZeroU32::MIN;

/// Nothing cancels a fetch yet; the UI's cancel button is ticket 09's.
static NEVER_INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The installer-managed clone, checked out at one Version at a time.
pub struct UpstreamCache {
    root: PathBuf,
    url: String,
}

impl UpstreamCache {
    /// `root` is the cache directory inside the App Data Store. It is created on first use.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            url: UPSTREAM_URL.to_owned(),
        }
    }

    /// Point the cache at a different repository.
    ///
    /// Only tests use this, against a fixture repository on disk — but it is public because
    /// the alternative is a `#[cfg(test)]` seam, and a fixture URL is not a secret.
    pub fn with_url(root: impl Into<PathBuf>, url: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            url: url.into(),
        }
    }

    /// Where the materialized working tree lives.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What the Version picker lists: every Release upstream offers, and `master`'s HEAD.
    ///
    /// One `ls-refs` round trip. No objects are transferred, so this is safe to call while
    /// the user is still deciding.
    pub fn list_versions(
        &self,
        progress: &ProgressReporter,
    ) -> Result<VersionCatalog, SourceError> {
        progress.report(Stage::Fetch, "Looking up the available versions.");
        let repo = self.open_or_init()?;
        let remote = repo
            .remote_at(self.url.as_str())
            .map_err(|err| self.unreachable(&err))?;
        let connection = remote
            .connect(Direction::Fetch)
            .map_err(|err| self.unreachable(&err))?;
        // Without this the remote is asked only for `refs/tags/`, because a remote with no
        // refspecs still carries the implicit "fetch all tags" one and its prefix is used to
        // filter the listing. The picker needs `master` as well.
        let options = gix::remote::ref_map::Options {
            prefix_from_spec_as_filter_on_remote: false,
            ..Default::default()
        };
        let (ref_map, _handshake) = connection
            .ref_map(gix::progress::Discard, options)
            .map_err(|err| self.unreachable(&err))?;

        let advertised: Vec<(String, String)> = ref_map
            .remote_refs
            .iter()
            .map(|remote_ref| {
                let (name, target, peeled) = remote_ref.unpack();
                let id = peeled
                    .or(target)
                    .map(|id| id.to_hex().to_string())
                    .unwrap_or_default();
                (name.to_string(), id)
            })
            .collect();
        let catalog = VersionCatalog::from_remote_refs(
            advertised
                .iter()
                .map(|(name, id)| (name.as_str(), id.clone())),
        );
        progress.report(
            Stage::Fetch,
            format!("Found {} releases.", catalog.releases().len()),
        );
        Ok(catalog)
    }

    /// Make `version` available on disk and return the root of the resulting tree.
    ///
    /// Fetch, then check out. Both steps are safe to interrupt: a failed fetch leaves the
    /// objects it did get and no ref pointing at a half-written state, and a checkout is only
    /// recorded as done once every file is written.
    pub fn materialize(
        &self,
        version: &Version,
        progress: &ProgressReporter,
    ) -> Result<PathBuf, SourceError> {
        let target = RefTarget::for_version(version);
        let repo = self.open_or_init()?;

        progress.report(
            Stage::Fetch,
            format!("Downloading {} — only what is new.", target.label),
        );
        self.fetch(&repo, &target)?;

        let commit = repo
            .find_reference(target.local.as_str())
            .map_err(|err| SourceError::VersionNotFound {
                version: target.label.clone(),
                detail: chain(&err),
            })?
            .peel_to_id()
            .map_err(|err| SourceError::CheckoutFailed {
                version: target.label.clone(),
                detail: chain(&err),
            })?
            .detach();

        self.checkout(&repo, commit, &target.label, progress)?;
        Ok(self.root.clone())
    }

    /// Open the cache repository, creating it if this is the first run.
    fn open_or_init(&self) -> Result<gix::Repository, SourceError> {
        fs::create_dir_all(&self.root).map_err(|err| SourceError::CacheUnusable {
            path: self.root.clone(),
            detail: err.to_string(),
        })?;
        let unusable = |err: &dyn std::error::Error| SourceError::CacheUnusable {
            path: self.root.clone(),
            detail: chain(err),
        };
        if self.root.join(".git").exists() {
            gix::open(&self.root).map_err(|err| unusable(&err))
        } else {
            gix::init(&self.root).map_err(|err| unusable(&err))
        }
    }

    /// Fetch exactly one commit for `target`, keeping every previously fetched Version.
    fn fetch(&self, repo: &gix::Repository, target: &RefTarget) -> Result<(), SourceError> {
        let refspec = format!("+{}:{}", target.remote, target.local);
        let remote = repo
            .remote_at(self.url.as_str())
            .map_err(|err| self.unreachable(&err))?
            .with_refspecs([refspec.as_str()], Direction::Fetch)
            .map_err(|err| SourceError::VersionNotFound {
                version: target.label.clone(),
                detail: chain(&err),
            })?
            // Tags are asked for by name when a Release is wanted. Fetching all of them with
            // every Version would drag in a snapshot per tag.
            .with_fetch_tags(Tags::None);

        let connection = remote
            .connect(Direction::Fetch)
            .map_err(|err| self.unreachable(&err))?;
        let prepared = connection
            .prepare_fetch(
                gix::progress::Discard,
                gix::remote::ref_map::Options::default(),
            )
            .map_err(|err| self.unreachable(&err))?;

        // The remote answered but had nothing matching: that is a wrong Version, not a
        // network problem, and it deserves the message that says so.
        if prepared.ref_map().mappings.is_empty() {
            return Err(SourceError::VersionNotFound {
                version: target.label.clone(),
                detail: format!("{} matched no ref on {}", target.remote, self.url),
            });
        }

        prepared
            .with_shallow(Shallow::DepthAtRemote(SHALLOW_DEPTH))
            .receive(gix::progress::Discard, &NEVER_INTERRUPTED)
            .map_err(|err| self.unreachable(&err))?;
        Ok(())
    }

    /// Write `commit`'s tree into the cache's working directory.
    ///
    /// Rewritten from scratch every time rather than updated in place: a directory that is
    /// emptied and refilled cannot keep a file from the Version before it, which is the same
    /// exactness Sync gives the game folders (rule 8).
    fn checkout(
        &self,
        repo: &gix::Repository,
        commit: gix::ObjectId,
        label: &str,
        progress: &ProgressReporter,
    ) -> Result<(), SourceError> {
        let marker = repo.git_dir().join(MATERIALIZED_MARKER);
        if fs::read_to_string(&marker).is_ok_and(|recorded| recorded.trim() == commit.to_string()) {
            progress.report(Stage::Fetch, format!("{label} is already unpacked."));
            return Ok(());
        }

        let failed = |detail: String| SourceError::CheckoutFailed {
            version: label.to_owned(),
            detail,
        };

        // Dropped first, so an interrupted checkout can never look finished.
        if marker.exists() {
            fs::remove_file(&marker).map_err(|err| failed(err.to_string()))?;
        }
        self.empty_working_tree().map_err(failed)?;

        progress.report(Stage::Fetch, format!("Unpacking {label}."));
        let tree = repo
            .find_object(commit)
            .map_err(|err| failed(chain(&err)))?
            .peel_to_tree()
            .map_err(|err| failed(chain(&err)))?
            .id;
        let mut index = repo
            .index_from_tree(&tree)
            .map_err(|err| failed(chain(&err)))?;
        let objects = repo
            .objects
            .clone()
            .into_arc()
            .map_err(|err| failed(chain(&err)))?;

        let options = gix::worktree::state::checkout::Options {
            // True because `empty_working_tree` just made it so.
            destination_is_initially_empty: true,
            // No filters and no attribute sources: blobs are written exactly as they are
            // stored. Rule 5 also means no `.gitattributes` in the repository may cause an
            // external filter program to be run.
            ..Default::default()
        };
        gix::worktree::state::checkout(
            &mut index,
            &self.root,
            objects,
            &gix::progress::Discard,
            &gix::progress::Discard,
            &NEVER_INTERRUPTED,
            options,
        )
        .map_err(|err| failed(chain(&err)))?;
        index
            .write(gix::index::write::Options::default())
            .map_err(|err| failed(chain(&err)))?;

        fs::write(&marker, commit.to_string()).map_err(|err| failed(err.to_string()))?;
        progress.report(Stage::Fetch, format!("{label} is ready."));
        Ok(())
    }

    /// Remove everything in the cache directory except the repository itself.
    fn empty_working_tree(&self) -> Result<(), String> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(err) => return Err(err.to_string()),
        };
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            let removed = if path.is_dir() && !path.is_symlink() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            removed.map_err(|err| format!("could not remove {}: {err}", path.display()))?;
        }
        Ok(())
    }

    fn unreachable(&self, error: &dyn std::error::Error) -> SourceError {
        SourceError::UpstreamUnreachable {
            url: self.url.clone(),
            detail: chain(error),
        }
    }
}
