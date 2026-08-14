//! Installation Sources for the Civ 5 VP Installer.
//!
//! This crate is the real implementation of the Core's first injected boundary,
//! [`civ5vp_core::SourceProvider`]. It exists as its own crate for the same reason the egui
//! shell does: the Core has no dependencies and must keep having none (rule 1), and an
//! embedded git implementation is a large dependency.
//!
//! Two Installation Sources, exactly as `CONTEXT.md` defines them:
//!
//! * the **Upstream Cache** — a managed clone of `LoneGazebo/Community-Patch-DLL`, fetched
//!   incrementally, with one **Version** checked out at a time;
//! * a **Local Repo** — a developer's own checkout, used as-is including uncommitted changes,
//!   with no git operation performed on it at all.
//!
//! The git work is done by `gix` (gitoxide) in-process. Nothing here runs an external program
//! (rule 5), so the installer works on a machine that has never had git installed.

// Rule 9: no panicking paths in code reachable from the UI. Crate-level, so the integration
// tests under `tests/` (separate crates) may `unwrap` as usual.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

mod error;
mod local;
mod provider;
mod upstream;
mod version;

pub use civ5vp_core::VersionCatalog;
pub use error::{LocalRepoProblem, SourceError};
pub use provider::InstallationSources;
pub use upstream::{UPSTREAM_URL, UpstreamCache};
