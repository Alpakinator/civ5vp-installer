//! The three Version tiers, and how each one maps onto a ref.
//!
//! `CONTEXT.md`: a Version is a **Release** (a `Release-*` tag), the **Latest Development
//! Version** (upstream `master` HEAD), or an **Arbitrary Ref**. This module owns the two
//! translations that follow from that: what the picker lists, and which ref a Version means.

use civ5vp_core::Version;

/// The prefix that makes a tag a Release.
const RELEASE_PREFIX: &str = "Release-";

/// The branch the Latest Development Version tracks.
const DEVELOPMENT_BRANCH: &str = "master";

/// What the Version picker shows, read straight off the upstream repository.
///
/// Nothing here is cached or guessed: it is the state of the remote at the moment
/// [`crate::UpstreamCache::list_versions`] was called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCatalog {
    releases: Vec<String>,
    latest_development_version: String,
}

impl VersionCatalog {
    /// Build a catalog from the raw ref names the remote advertised.
    ///
    /// `refs` is `(full ref name, object id)`. Anything that is not a `Release-*` tag or the
    /// development branch is ignored — an Arbitrary Ref is typed in, never listed.
    pub(crate) fn from_remote_refs<'a>(refs: impl IntoIterator<Item = (&'a str, String)>) -> Self {
        let mut releases = Vec::new();
        let mut latest_development_version = String::new();
        for (name, id) in refs {
            if let Some(tag) = name.strip_prefix("refs/tags/") {
                // `^{}` is how a peeled annotated tag is advertised; the tag name itself is
                // already in the list, so the peeled entry is a duplicate.
                if tag.ends_with("^{}") {
                    continue;
                }
                if tag.starts_with(RELEASE_PREFIX) {
                    releases.push(tag.to_owned());
                }
            } else if name == format!("refs/heads/{DEVELOPMENT_BRANCH}") {
                latest_development_version = id;
            }
        }
        releases.sort_by_key(|tag| std::cmp::Reverse(release_order(tag)));
        releases.dedup();
        Self {
            releases,
            latest_development_version,
        }
    }

    /// Every Release upstream offers, newest first.
    ///
    /// Each entry is the tag name, which is also the payload of [`Version::Release`].
    pub fn releases(&self) -> &[String] {
        &self.releases
    }

    /// The newest Release, which is what the picker should select by default.
    pub fn newest_release(&self) -> Option<Version> {
        self.releases.first().cloned().map(Version::Release)
    }

    /// The commit `master` currently points at.
    ///
    /// Empty if the remote has no `master`, which upstream always does — a fixture repository
    /// used by a test may not.
    pub fn latest_development_version(&self) -> &str {
        &self.latest_development_version
    }
}

/// Sort key for a Release tag: its dotted numbers, then the raw name as a tiebreaker.
///
/// `Release-1.10` has to come after `Release-1.9`, which it does not do as a string, so the
/// numbers are compared as numbers. Anything unparsable sorts below everything numbered
/// rather than being dropped.
fn release_order(tag: &str) -> (Vec<u64>, String) {
    let numbers = tag
        .strip_prefix(RELEASE_PREFIX)
        .unwrap_or(tag)
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    (numbers, tag.to_owned())
}

/// Where a Version lives on the remote, and where the Upstream Cache keeps it locally.
///
/// Every materialized Version keeps its own local ref. That is not bookkeeping for its own
/// sake: the local refs are what the next fetch offers the server as "already have", and they
/// are the reason switching Version transfers a fraction of a first fetch instead of a whole
/// second snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefTarget {
    /// What to ask the remote for — a full ref name, a short name, or a commit id.
    pub(crate) remote: String,
    /// The local ref the fetched commit is written to.
    pub(crate) local: String,
    /// How the Version reads in an error message.
    pub(crate) label: String,
}

impl RefTarget {
    pub(crate) fn for_version(version: &Version) -> Self {
        match version {
            Version::Release(name) => {
                let tag = release_tag_name(name);
                Self {
                    remote: format!("refs/tags/{tag}"),
                    local: format!("refs/civ5vp/tags/{tag}"),
                    label: tag,
                }
            }
            Version::LatestDevelopmentVersion => Self {
                remote: format!("refs/heads/{DEVELOPMENT_BRANCH}"),
                local: format!("refs/civ5vp/heads/{DEVELOPMENT_BRANCH}"),
                label: format!("the latest development version ({DEVELOPMENT_BRANCH})"),
            },
            // Left as typed: a short name matches a branch or a tag on the remote the same way
            // `git fetch origin <name>` does, and a commit id is asked for by id.
            Version::ArbitraryRef(reference) => Self {
                remote: reference.clone(),
                local: format!("refs/civ5vp/arbitrary/{}", sanitize(reference)),
                label: reference.clone(),
            },
        }
    }
}

/// The tag name for a Release, whether or not the caller included the prefix.
///
/// The picker hands back what [`VersionCatalog::releases`] listed, which always has it. A
/// version typed or restored from settings might be just `5.4.2`.
fn release_tag_name(name: &str) -> String {
    if name.starts_with(RELEASE_PREFIX) {
        name.to_owned()
    } else {
        format!("{RELEASE_PREFIX}{name}")
    }
}

/// Reduce an Arbitrary Ref to something that is certainly a legal single ref component.
///
/// Only the local bookkeeping ref goes through this; what is sent to the remote is what the
/// user typed.
fn sanitize(reference: &str) -> String {
    let cleaned: String = reference
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();
    // A ref component may not start with a dot or be empty.
    let trimmed = cleaned.trim_start_matches('.');
    if trimmed.is_empty() {
        "ref".to_owned()
    } else {
        trimmed.to_owned()
    }
}
