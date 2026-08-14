//! What the Version picker lists: the catalog of Versions the Upstream Cache can offer.
//!
//! `CONTEXT.md`: a Version is a **Release** (a `Release-*` tag), the **Latest Development
//! Version** (upstream `master` HEAD), or an **Arbitrary Ref** (typed in, never listed).
//! The catalog is a boundary type — the source provider fills it (from a real `ls-refs` in
//! production, from fixtures in tests), the shell draws it, and the Core passes it through
//! without keeping it: nothing here is cached or guessed.

use crate::configuration::Version;

/// The prefix that makes a tag a Release.
const RELEASE_PREFIX: &str = "Release-";

/// The branch the Latest Development Version tracks.
const DEVELOPMENT_BRANCH: &str = "master";

/// The Versions on offer, read straight off the upstream repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCatalog {
    releases: Vec<String>,
    latest_development_version: String,
}

impl VersionCatalog {
    /// Build a catalog from the raw ref names a remote advertised.
    ///
    /// `refs` is `(full ref name, object id)`. Anything that is not a `Release-*` tag or the
    /// development branch is ignored — an Arbitrary Ref is typed in, never listed.
    pub fn from_remote_refs<'a>(refs: impl IntoIterator<Item = (&'a str, String)>) -> Self {
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

    /// The newest Release, which is what the picker selects by default.
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
