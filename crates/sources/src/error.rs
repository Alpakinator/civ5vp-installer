//! Typed errors for the Installation Sources.
//!
//! Same shape as the Core's: `user_message` is the sentence the UI shows, `log_detail` is
//! where raw git and IO text is allowed to appear (rules 10 and 11). Crossing back into the
//! Core happens through [`BoundaryError`], which carries exactly those two strings.

use std::fmt;
use std::path::PathBuf;

use civ5vp_core::BoundaryError;

/// Why a Local Repo cannot be used as an Installation Source.
///
/// Deliberately short: the installer does not inspect a Local Repo beyond checking that the
/// path it was handed is a real place on disk. Whether the checkout contains the mod folders
/// is the Core's question, and it already answers it with a better message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalRepoProblem {
    /// A relative path, which would be resolved against the working directory.
    NotAbsolute,
    /// Nothing there, or there but not a directory.
    NotADirectory,
}

/// Anything that can stop an Installation Source from being materialized.
#[derive(Debug)]
pub enum SourceError {
    /// The Upstream Cache directory itself could not be created or opened.
    CacheUnusable { path: PathBuf, detail: String },
    /// The upstream repository could not be reached or refused the request.
    UpstreamUnreachable { url: String, detail: String },
    /// The requested Version does not exist upstream.
    VersionNotFound { version: String, detail: String },
    /// The Version was fetched but its files could not be written into the cache.
    CheckoutFailed { version: String, detail: String },
    /// The Local Repo path cannot be used.
    LocalRepoUnusable {
        path: PathBuf,
        problem: LocalRepoProblem,
    },
}

impl SourceError {
    /// The sentence shown in the UI.
    pub fn user_message(&self) -> String {
        match self {
            Self::CacheUnusable { path, .. } => format!(
                "The installer could not use its download folder at {}. Check that the folder \
                 exists and is not read-only.",
                path.display()
            ),
            Self::UpstreamUnreachable { .. } => {
                "Could not reach the Vox Populi source repository. Check your internet \
                 connection and try again — nothing has been changed."
                    .to_owned()
            }
            Self::VersionNotFound { version, .. } => format!(
                "There is no version called \"{version}\" in the Vox Populi repository. Pick one \
                 from the list, or check the spelling."
            ),
            Self::CheckoutFailed { .. } => {
                "The mod files were downloaded but could not be unpacked. Check that you have \
                 free disk space and try again."
                    .to_owned()
            }
            Self::LocalRepoUnusable { path, problem } => match problem {
                LocalRepoProblem::NotAbsolute => format!(
                    "Your own copy of the repository needs to be a full path starting from the \
                     root of the drive, not \"{}\".",
                    path.display()
                ),
                LocalRepoProblem::NotADirectory => format!(
                    "There is no folder at {}. Pick your copy of the Community-Patch-DLL \
                     repository again.",
                    path.display()
                ),
            },
        }
    }

    /// The full detail, for the log file (rule 11).
    pub fn log_detail(&self) -> String {
        match self {
            Self::CacheUnusable { path, detail } => {
                format!("upstream cache at {} unusable: {detail}", path.display())
            }
            Self::UpstreamUnreachable { url, detail } => {
                format!("fetch from {url} failed: {detail}")
            }
            Self::VersionNotFound { version, detail } => {
                format!("version {version} not found upstream: {detail}")
            }
            Self::CheckoutFailed { version, detail } => {
                format!("checkout of {version} failed: {detail}")
            }
            Self::LocalRepoUnusable { path, problem } => {
                format!("local repo rejected: {problem:?} at {}", path.display())
            }
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for SourceError {}

impl From<SourceError> for BoundaryError {
    fn from(error: SourceError) -> Self {
        BoundaryError::new(error.user_message(), error.log_detail())
    }
}

/// Flatten an error and everything it was caused by into one log line.
///
/// `gix`'s errors nest several layers deep and the outermost one is usually the least
/// informative ("failed to fetch"), so the whole chain goes to the log.
pub(crate) fn chain(error: &dyn std::error::Error) -> String {
    let mut detail = error.to_string();
    let mut cause = error.source();
    while let Some(current) = cause {
        detail.push_str(": ");
        detail.push_str(&current.to_string());
        cause = current.source();
    }
    detail
}
