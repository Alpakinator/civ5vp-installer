//! One error type for the whole bootstrap: a sentence for the user, everything else for the
//! log.

use std::fmt;
use std::path::Path;

use civ5vp_core::BoundaryError;

/// A Toolchain Bootstrap failure.
///
/// The shape is deliberately the same as [`BoundaryError`]'s - a sentence for the user and
/// everything else for the log - because that is what this crate hands back across the Core
/// seam. It is a separate type only so the bootstrap's internals can build errors without
/// pretending every internal step is a boundary failure.
#[derive(Debug, Clone)]
pub struct ToolchainError {
    message: String,
    detail: String,
}

impl ToolchainError {
    /// `message` is read by a player; `detail` is read by whoever they send the log to.
    pub fn new(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            detail: detail.into(),
        }
    }

    /// A sentence a non-programmer can act on.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Full detail - the IO error, the byte offset, the URL, the hash that did not match.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Add another line of context to the log side, leaving the user-facing sentence alone.
    ///
    /// This is how a low-level "unexpected end of file" turns into something a maintainer can
    /// place, without the user's message drifting into jargon as it travels up the stack.
    pub fn context(mut self, detail: impl fmt::Display) -> Self {
        self.detail = format!("{detail}: {}", self.detail);
        self
    }
}

impl fmt::Display for ToolchainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolchainError {}

impl From<ToolchainError> for BoundaryError {
    fn from(error: ToolchainError) -> Self {
        BoundaryError::new(error.message, error.detail)
    }
}

/// The message every IO failure inside the Toolchain Cache gets. There is exactly one thing
/// a user can do about a failed write, so there is exactly one sentence.
const IO_MESSAGE: &str = "The installer could not write the toolchain files. Check you have free disk space and \
     that the installer's data folder is writable.";

/// Wrap an IO error against the path it happened on.
pub fn io_error(action: &str, path: &Path, error: &std::io::Error) -> ToolchainError {
    ToolchainError::new(
        IO_MESSAGE,
        format!("failed to {action} {}: {error}", path.display()),
    )
}

/// Wrap an IO error that has no single path - a stream, a decoder, an in-memory reader.
pub fn stream_error(action: &str, error: &std::io::Error) -> ToolchainError {
    ToolchainError::new(
        "The installer could not read the downloaded toolchain files. They may be damaged; \
         clearing the installer's data folder and retrying will download them again.",
        format!("failed to {action}: {error}"),
    )
}

/// A file the extraction contract promised is not in the archive.
///
/// Its own constructor because this is the failure that means "the artifact is not the one
/// `docs/pinned-artifacts.md` describes", which is a different problem from a bad disk.
pub fn missing_member(what: &str, where_: &str) -> ToolchainError {
    ToolchainError::new(
        "The Windows SDK download is not the file the installer expected. Clear the \
         installer's data folder and try again.",
        format!("{what} is missing from {where_}"),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn context_accumulates_on_the_log_side_only() {
        let error = ToolchainError::new("Plain sentence.", "root cause")
            .context("reading cab1.cab")
            .context("extracting the SDK");

        assert_eq!(error.message(), "Plain sentence.");
        assert_eq!(
            error.detail(),
            "extracting the SDK: reading cab1.cab: root cause"
        );
    }

    #[test]
    fn crossing_the_core_seam_keeps_both_halves() {
        let boundary: BoundaryError = ToolchainError::new("Plain sentence.", "root cause").into();

        assert_eq!(boundary.message(), "Plain sentence.");
        assert_eq!(boundary.detail(), "root cause");
    }
}
