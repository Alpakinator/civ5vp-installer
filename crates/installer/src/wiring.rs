//! The real Core: both boundaries wired to their production implementations.
//!
//! This is the composition root - the one place the installer decides that "sources" means
//! the Upstream Cache / Local Repo provider from `civ5vp-sources` and that "build" means the
//! bootstrapped clang from `civ5vp-toolchain`. The shell tests wire
//! [`crate::placeholder`] instead, which is what keeps the fast suite offline;
//! this module is exercised by the `#[ignore]`d real-install test and by the shipped binary.

use std::path::Path;

use civ5vp_core::{AppDataStore, Core};
use civ5vp_sources::{InstallationSources, LuaJitCache, UPSTREAM_URL, UpstreamCache};
use civ5vp_toolchain::BootstrappedToolchain;

/// A Core over the real boundaries, everything installer-owned inside the App Data Store.
pub fn core(store: &AppDataStore) -> Core {
    core_at(store.root())
}

/// The same wiring against an explicit root - how the real-install test keeps its gigabytes
/// in a directory of its own choosing.
pub fn core_at(root: &Path) -> Core {
    let upstream = UpstreamCache::new(root.join("upstream-cache"), UPSTREAM_URL);
    // Its own directory beside the Upstream Cache: the two are fetched from different remotes
    // and have different lifetimes, and the Upstream Cache empties its working tree on every
    // Version switch.
    let luajit = LuaJitCache::new(root.join("luajit-cache"));
    Core::new(
        Box::new(InstallationSources::new(upstream, luajit)),
        Box::new(BootstrappedToolchain::new(root.join("toolchain-cache"))),
        Box::new(civ5vp_modpack::SqliteModpackAssembler::new()),
        root.to_path_buf(),
    )
}
