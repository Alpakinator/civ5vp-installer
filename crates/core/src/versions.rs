//! What the Version picker lists: the catalog of Versions the Upstream Cache can offer.
//!
//! `CONTEXT.md`: a Version is a **Release** (a `Release-*` tag), the **Latest Development
//! Version** (upstream `master` HEAD), or an **Arbitrary Ref** (typed in, never listed).
//! The catalog is a boundary type - the source provider fills it (from a real `ls-refs` in
//! production, from fixtures in tests), the shell draws it, and the Core passes it through
//! without keeping it: nothing here is cached or guessed.

use crate::configuration::Version;

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
    /// development branch is ignored - an Arbitrary Ref is typed in, never listed.
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
    /// Empty if the remote has no `master`, which upstream always does - a fixture repository
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

/// One commit after the newest Release, as the unofficial-versions list offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnofficialVersion {
    /// `5.4.3.07` - the newest Release's numbers plus this commit's position after it.
    pub label: String,
    /// The commit message's first line, however long - the shell decides how to fit it.
    pub summary: String,
    /// The full commit hash, which is what actually gets installed.
    pub commit: String,
}

/// What changing Version does to the saves a player already has.
///
/// Vox Populi breaks save compatibility whenever the first or second number moves:
/// `5.4.4` to `5.4.5` is safe, `5.4.5` to `5.5.0` is not. The third number and anything after
/// it are fixes within a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveCompatibility {
    /// Same line, or nothing installed to compare against. Say nothing.
    Unaffected,
    /// Both sides carry version numbers and the line changed.
    Broken { installed: String, chosen: String },
    /// At least one side is `master`, an arbitrary ref, or a Local Repo, so there is no
    /// number to compare. Distinct from [`Self::Unaffected`] on purpose: silence reads as
    /// "you are fine", and that is the one thing this cannot promise.
    Unknown { installed: String, chosen: String },
}

impl SaveCompatibility {
    /// Compare what is installed against what is about to be.
    pub fn between(installed: &str, chosen: &str) -> Self {
        if installed == chosen {
            return Self::Unaffected;
        }
        match (line_of(installed), line_of(chosen)) {
            (Some(was), Some(now)) if was == now => Self::Unaffected,
            (Some(_), Some(_)) => Self::Broken {
                installed: installed.to_owned(),
                chosen: chosen.to_owned(),
            },
            // One side is a branch, a commit, or a developer's own checkout. `master` is the
            // case that matters: it sits ahead of the newest Release, so moving off it is a
            // downgrade across a boundary no number can describe.
            _ => Self::Unknown {
                installed: installed.to_owned(),
                chosen: chosen.to_owned(),
            },
        }
    }

    /// The sentence to show, or `None` when there is nothing to say.
    ///
    /// States the fact and stops. What to do about it - finish the current game, keep the old
    /// install - is the player's business, and a banner that gave instructions would be
    /// telling people who already know.
    pub fn message(&self) -> Option<String> {
        match self {
            Self::Unaffected => None,
            Self::Broken { installed, chosen } => Some(format!(
                "Save games are not compatible between {installed} and {chosen}."
            )),
            Self::Unknown { installed, chosen } => Some(format!(
                "Save compatibility between {installed} and {chosen} is not known."
            )),
        }
    }
}

/// The `major.minor` of a Version label, when it has one.
///
/// Handles every shape upstream actually publishes: `Release-5.4.5`, `Release-5.2` with no
/// third component at all, and unofficial builds labelled `5.4.3.07`. Returns `None` for
/// `master`, `Local`, and arbitrary refs - they carry no version to compare.
fn line_of(label: &str) -> Option<(u32, u32)> {
    let digits = label.strip_prefix(RELEASE_PREFIX).unwrap_or(label);
    let mut parts = digits.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}
