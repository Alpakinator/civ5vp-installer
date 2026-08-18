//! The three Version tiers, and how each one maps onto a ref.
//!
//! `CONTEXT.md`: a Version is a **Release** (a `Release-*` tag), the **Latest Development
//! Version** (upstream `master` HEAD), or an **Arbitrary Ref**. This module owns the two
//! translations that follow from that: what the picker lists, and which ref a Version means.

use civ5vp_core::Version;

const RELEASE_PREFIX: &str = "Release-";

/// The branch the Latest Development Version tracks.
const DEVELOPMENT_BRANCH: &str = "master";

/// Where a Version lives on the remote, and where the Upstream Cache keeps it locally.
///
/// Every materialized Version keeps its own local ref: the local refs are what the next fetch
/// offers the server as "already have", and they are the reason switching Version transfers a
/// fraction of a first fetch instead of a whole second snapshot.
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
            // The unofficial-versions list recorded the commit when it was fetched, so the
            // label stays honest however far upstream has moved since.
            Version::UnofficialBuild { label, commit } => Self {
                remote: commit.clone(),
                local: format!("refs/civ5vp/unofficial/{}", sanitize(commit)),
                label: label.clone(),
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
