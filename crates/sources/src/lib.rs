//! Installation Sources for the Civ 5 VP Installer.
//!
//! This crate is the real implementation of the Core's first injected boundary,
//! [`civ5vp_core::SourceProvider`]. It exists as its own crate because the Core has no
//! dependencies and must keep having none, and an embedded git implementation is a large
//! dependency.
//!
//! Two Installation Sources, exactly as `CONTEXT.md` defines them:
//!
//! * the **Upstream Cache** — a managed clone of `LoneGazebo/Community-Patch-DLL`, fetched
//!   incrementally, with one **Version** checked out at a time;
//! * a **Local Repo** — a developer's own checkout, used as-is including uncommitted changes,
//!   with no git operation performed on it at all.
//!
//! Beside them sits one more fetched thing, which is not an Installation Source because the
//! player never chooses it: the **pinned LuaJIT checkout** (see [`LuaJitCache`]), fetched only
//! when the configuration opts into the LuaJIT engine.
//!
//! The git work is done by `gix` (gitoxide) in-process. Nothing here runs an external
//! program, so the installer works on a machine that has never had git installed.

// No panicking paths in code reachable from the UI. Crate-level, so the integration tests
// under `tests/` (separate crates) may `unwrap` as usual.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

mod error;
mod local;
mod luajit;
mod provider;
mod upstream;
mod version;

pub use civ5vp_core::VersionCatalog;
pub use error::{LocalRepoProblem, SourceError};
pub use luajit::{LUAJIT_COMMIT, LUAJIT_URL, LuaJitCache};
pub use provider::InstallationSources;
pub use upstream::{UPSTREAM_URL, UpstreamCache};
