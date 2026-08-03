//! Typed errors. Every one carries a sentence for the user and the detail for the log.

use std::fmt;
use std::path::PathBuf;

use crate::boundaries::BoundaryError;

/// Anything that can stop a Deployment.
///
/// Rule 10: `user_message` is what the UI shows — a sentence a non-programmer can act on.
/// `log_detail` is what goes in the log file, and is where raw compiler, git, and IO text
/// is allowed to appear.
#[derive(Debug)]
pub enum InstallError {
    /// The source provider could not materialize the Installation Source.
    Fetch(BoundaryError),
    /// The toolchain runner could not produce the Built DLL.
    Build(BoundaryError),
    /// The toolchain runner reported success but wrote no DLL.
    MissingBuiltDll { expected: PathBuf },
    /// A file operation against the game folders failed.
    Deployment {
        /// What was being attempted, e.g. "copy" — used to build the log line.
        action: &'static str,
        path: PathBuf,
        cause: std::io::Error,
    },
    /// The Installation Source does not contain a folder the configuration needs.
    MissingInSource { folder_name: String, path: PathBuf },
    /// A configuration this build of the installer cannot deploy yet.
    UnsupportedConfiguration { message: String, detail: String },
}

impl InstallError {
    /// The sentence shown in the UI.
    pub fn user_message(&self) -> String {
        match self {
            Self::Fetch(err) => err.message().to_owned(),
            Self::Build(err) => format!(
                "{} You can try installing a Release instead, which is the most tested option.",
                err.message()
            ),
            Self::MissingBuiltDll { .. } => {
                "The DLL build finished but produced no file, so nothing was installed. \
                 Your game is unchanged."
                    .to_owned()
            }
            Self::Deployment { path, .. } => format!(
                "Could not write to {}. Check that the folder exists and is not read-only.",
                path.display()
            ),
            Self::MissingInSource { folder_name, .. } => format!(
                "The sources are missing the \"{folder_name}\" folder, so there is nothing to \
                 install. Your game is unchanged."
            ),
            Self::UnsupportedConfiguration { message, .. } => message.clone(),
        }
    }

    /// The full detail, for the log file (rule 11).
    pub fn log_detail(&self) -> String {
        match self {
            Self::Fetch(err) => format!("fetch failed: {}", err.detail()),
            Self::Build(err) => format!("build failed: {}", err.detail()),
            Self::MissingBuiltDll { expected } => {
                format!(
                    "toolchain runner returned Ok but {} does not exist",
                    expected.display()
                )
            }
            Self::Deployment {
                action,
                path,
                cause,
            } => format!("deployment: {action} {} failed: {cause}", path.display()),
            Self::MissingInSource { folder_name, path } => format!(
                "installation source has no \"{folder_name}\" at {}",
                path.display()
            ),
            Self::UnsupportedConfiguration { detail, .. } => detail.clone(),
        }
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Deployment { cause, .. } => Some(cause),
            _ => None,
        }
    }
}
