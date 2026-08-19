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
//!     .git/                        the managed repository - objects for every Version ever fetched
//!     .git/civ5vp-materialized     the commit the tree beside it was written from
//!     (1) Community Patch/…        the working tree of the selected Version
//! ```
//!
//! Objects accumulate and are never pruned. That is the whole incrementality story: the local
//! refs under `refs/civ5vp/` are what the next fetch offers the server as "already have", so
//! content that is the same between two Versions is not sent twice.

use std::fs;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use civ5vp_core::{MaterializedSource, ProgressReporter, Stage, Version};
use gix::remote::Direction;
use gix::remote::fetch::{Shallow, Tags};

use crate::error::{SourceError, chain};
use civ5vp_core::VersionCatalog;

use crate::version::RefTarget;

/// The one network source the installer has for mod files (`docs/pinned-artifacts.md` §6).
pub const UPSTREAM_URL: &str = "https://github.com/LoneGazebo/Community-Patch-DLL.git";

/// Written inside the cache's own `.git` when this code creates it, and checked before the
/// working tree is emptied. Its only job is to make "is this directory ours?" answerable.
/// It lives under `.git` because the working tree is what gets deployed, and `.git` is the
/// one thing `empty_working_tree` preserves.
const CACHE_MARKER: &str = ".git/civ5vp-upstream-cache";

const CACHE_MARKER_CONTENTS: &str = "This folder is the Civ 5 VP Installer's Upstream Cache. It is safe to delete when the \
     installer is not running; the installer will fetch what it needs again.\n";

/// Records which commit the working tree beside it was written from. Kept inside `.git` so it
/// never shows up in the tree the Core walks.
const MATERIALIZED_MARKER: &str = "civ5vp-materialized";

/// One commit per Version and nothing behind it - the whole point of the strategy.
const SHALLOW_DEPTH: NonZeroU32 = NonZeroU32::MIN;

/// Nothing cancels a fetch yet.
static NEVER_INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The installer-managed clone, checked out at one Version at a time.
pub struct UpstreamCache {
    root: PathBuf,
    url: String,
    /// Where the unofficial-versions list comes from: a GitHub-compare-shaped endpoint,
    /// derived from `url` unless overridden.
    compare_api: Option<String>,
}

impl UpstreamCache {
    /// `root` is the cache directory inside the App Data Store, created on first use; `url`
    /// is the repository to fetch from, which is [`UPSTREAM_URL`] everywhere except in tests.
    pub fn new(root: impl Into<PathBuf>, url: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            url: url.into(),
            compare_api: None,
        }
    }

    /// Point the unofficial-versions listing at a different compare endpoint - a mirror,
    /// or a test's fixture server. The default is derived from the repository URL:
    /// `https://github.com/OWNER/REPO(.git)` → `https://api.github.com/repos/OWNER/REPO/compare`.
    pub fn with_compare_api(mut self, endpoint: impl Into<String>) -> Self {
        self.compare_api = Some(endpoint.into());
        self
    }

    fn compare_api(&self) -> Result<String, SourceError> {
        if let Some(endpoint) = &self.compare_api {
            return Ok(endpoint.clone());
        }
        let rest = self
            .url
            .strip_prefix("https://github.com/")
            .or_else(|| self.url.strip_prefix("http://github.com/"));
        match rest {
            Some(path) => {
                let path = path.trim_end_matches('/').trim_end_matches(".git");
                Ok(format!("https://api.github.com/repos/{path}/compare"))
            }
            None => Err(SourceError::UpstreamUnreachable {
                url: self.url.clone(),
                detail: "unofficial versions need a GitHub upstream (no compare endpoint)"
                    .to_owned(),
            }),
        }
    }

    /// Every commit after `newest_release`, oldest first, labelled `X.Y.Z.NN`.
    ///
    /// One GitHub compare call - the Upstream Cache is a shallow clone with no history to
    /// walk locally. The endpoint reports at most 250 commits per page; if upstream is
    /// further ahead than that, the newest are missing until the next Release, and the
    /// progress line says so.
    pub fn list_unofficial(
        &self,
        newest_release: &str,
        progress: &ProgressReporter,
    ) -> Result<Vec<civ5vp_core::UnofficialVersion>, SourceError> {
        progress.report(
            Stage::Fetch,
            "Looking up the changes since the newest release.",
        );
        let tag = if newest_release.starts_with("Release-") {
            newest_release.to_owned()
        } else {
            format!("Release-{newest_release}")
        };
        let url = format!("{}/{tag}...master?per_page=250", self.compare_api()?);
        let unreachable = |detail: String| SourceError::UpstreamUnreachable {
            url: url.clone(),
            detail,
        };
        let mut response = ureq::get(&url)
            // GitHub's API refuses requests without a User-Agent.
            .header("User-Agent", "civ5vp-installer")
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|error| unreachable(error.to_string()))?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| unreachable(error.to_string()))?;
        let (versions, total) =
            parse_compare(&body, tag.trim_start_matches("Release-")).map_err(unreachable)?;
        if total > versions.len() as u64 {
            progress.report(
                Stage::Fetch,
                format!(
                    "Upstream is {total} changes ahead - listing the oldest {}.",
                    versions.len()
                ),
            );
        } else {
            progress.report(
                Stage::Fetch,
                format!("Found {} changes since {tag}.", versions.len()),
            );
        }
        Ok(versions)
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

    /// Make `version` available on disk and return the resulting tree with its identity.
    ///
    /// Fetch, then check out. Both steps are safe to interrupt: a failed fetch leaves the
    /// objects it did get and no ref pointing at a half-written state, and a checkout is only
    /// recorded as done once every file is written.
    ///
    /// The `source_identity` is the checked-out commit's *tree* id: no file content is
    /// re-hashed, and two refs pointing at identical trees (an amend, a rebase) share an
    /// identity instead of forcing a needless rebuild.
    pub fn materialize(
        &self,
        version: &Version,
        progress: &ProgressReporter,
    ) -> Result<MaterializedSource, SourceError> {
        let target = RefTarget::for_version(version);
        let repo = self.open_or_init()?;

        progress.report(
            Stage::Fetch,
            format!("Downloading {} - only what is new.", target.label),
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

        let tree = (|| -> Result<String, String> {
            let object = repo.find_object(commit).map_err(|err| chain(&err))?;
            let found = object.peel_to_commit().map_err(|err| chain(&err))?;
            Ok(found.tree_id().map_err(|err| chain(&err))?.to_string())
        })()
        .map_err(|detail| SourceError::CheckoutFailed {
            version: target.label.clone(),
            detail,
        })?;

        self.checkout(&repo, commit, &target.label, progress)?;
        Ok(MaterializedSource {
            root: self.root.clone(),
            source_identity: format!("git-tree:{tree}"),
        })
    }

    fn open_or_init(&self) -> Result<gix::Repository, SourceError> {
        fs::create_dir_all(&self.root).map_err(|err| SourceError::CacheUnusable {
            path: self.root.clone(),
            detail: err.to_string(),
        })?;
        let unusable = |err: &dyn std::error::Error| SourceError::CacheUnusable {
            path: self.root.clone(),
            detail: chain(err),
        };
        let repository = if self.root.join(".git").exists() {
            gix::open(&self.root).map_err(|err| unusable(&err))
        } else {
            gix::init(&self.root).map_err(|err| unusable(&err))
        }?;

        // Written as soon as the repository exists; `empty_working_tree` checks it before
        // deleting anything.
        let marker = self.root.join(CACHE_MARKER);
        if !marker.is_file() {
            fs::write(&marker, CACHE_MARKER_CONTENTS).map_err(|err| {
                SourceError::CacheUnusable {
                    path: self.root.clone(),
                    detail: err.to_string(),
                }
            })?;
        }
        Ok(repository)
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

        // The remote answered but had nothing matching: a wrong Version, not a network problem.
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
    /// emptied and refilled cannot keep a file from the Version before it.
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
            // No filters and no attribute sources: blobs are written exactly as stored, and
            // no `.gitattributes` in the repository may cause an external filter program to run.
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
    ///
    /// This deletes a whole directory tree, so it first checks the directory is one this code
    /// made: `new` accepts any path, and a caller that passed a wrong one - the user's home
    /// directory, say - would otherwise have it emptied.
    fn empty_working_tree(&self) -> Result<(), String> {
        if !self.root.join(CACHE_MARKER).is_file() {
            return Err(format!(
                "{} is not an Upstream Cache this installer created ({CACHE_MARKER} is missing), \
                 so it will not be emptied",
                self.root.display()
            ));
        }
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

/// The compare response, reduced to what the picker needs: the commits after the base tag
/// in chronological order, labelled `<base>.NN`, plus the total upstream reported (which
/// exceeds the listed count when upstream is further ahead than one page).
fn parse_compare(
    body: &str,
    base: &str,
) -> Result<(Vec<civ5vp_core::UnofficialVersion>, u64), String> {
    let parsed: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("compare response was not JSON: {error}"))?;
    let commits = parsed
        .get("commits")
        .and_then(|commits| commits.as_array())
        .ok_or_else(|| "compare response had no commits list".to_owned())?;
    let mut versions = Vec::new();
    for (index, entry) in commits.iter().enumerate() {
        let Some(sha) = entry.get("sha").and_then(|sha| sha.as_str()) else {
            continue;
        };
        let message = entry
            .pointer("/commit/message")
            .and_then(|message| message.as_str())
            .unwrap_or("");
        versions.push(civ5vp_core::UnofficialVersion {
            label: format!("{base}.{:02}", index + 1),
            summary: message.lines().next().unwrap_or("").trim().to_owned(),
            commit: sha.to_owned(),
        });
    }
    let total = parsed
        .get("total_commits")
        .and_then(|total| total.as_u64())
        .unwrap_or(versions.len() as u64);
    Ok((versions, total))
}

#[cfg(test)]
// The crate-level deny is for code the UI can reach; tests may unwrap.
#[allow(clippy::unwrap_used)]
mod unofficial_tests {
    use super::parse_compare;

    /// A trimmed-down GitHub compare response: two commits, the second with a multi-line
    /// message full of the escapes a hand parser would fumble.
    const COMPARE: &str = r#"{
        "total_commits": 5,
        "commits": [
            {"sha": "aaaa000000000000000000000000000000000000",
             "commit": {"message": "Fix a promotion"}},
            {"sha": "bbbb000000000000000000000000000000000000",
             "commit": {"message": "Say \"hello\" to <everyone>\n\nDetails follow."}}
        ]
    }"#;

    #[test]
    fn commits_become_numbered_versions_in_order() {
        let (versions, total) = parse_compare(COMPARE, "5.4.3").unwrap();
        assert_eq!(total, 5);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].label, "5.4.3.01");
        assert_eq!(versions[0].summary, "Fix a promotion");
        assert_eq!(
            versions[0].commit,
            "aaaa000000000000000000000000000000000000"
        );
        assert_eq!(versions[1].label, "5.4.3.02");
    }

    #[test]
    fn the_summary_is_the_first_line_with_escapes_resolved() {
        let (versions, _) = parse_compare(COMPARE, "5.4.3").unwrap();
        assert_eq!(versions[1].summary, "Say \"hello\" to <everyone>");
    }

    #[test]
    fn a_response_that_is_not_a_compare_is_an_error() {
        assert!(parse_compare("[]", "5.4.3").is_err());
        assert!(parse_compare("not json", "5.4.3").is_err());
    }
}
