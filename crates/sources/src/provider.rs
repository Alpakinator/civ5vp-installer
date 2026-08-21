//! The real [`SourceProvider`].

use std::path::PathBuf;

use civ5vp_core::{
    BoundaryError, InstallationSource, MaterializedSource, ProgressReporter, SourceProvider,
};

use crate::local;
use crate::luajit::LuaJitCache;
use crate::upstream::UpstreamCache;

/// Both Installation Sources, because the Core sees them as one boundary.
///
/// The Upstream Cache is a managed clone that has to be fetched and checked out; a Local Repo
/// is handed straight back. Which of the two applies is decided by the
/// [`InstallationSource`] the Core passes in, not by configuration here.
///
/// The LuaJIT checkout rides along on the same boundary rather than getting one of its own: it
/// is source code fetched from a git remote into the App Data Store, which is exactly what this
/// boundary already means.
pub struct InstallationSources {
    upstream: UpstreamCache,
    luajit: LuaJitCache,
}

impl InstallationSources {
    pub fn new(upstream: UpstreamCache, luajit: LuaJitCache) -> Self {
        Self { upstream, luajit }
    }
}

impl SourceProvider for InstallationSources {
    fn materialize(
        &self,
        source: &InstallationSource,
        progress: &ProgressReporter,
    ) -> Result<MaterializedSource, BoundaryError> {
        match source {
            InstallationSource::UpstreamCache { version } => {
                self.upstream.materialize(version, progress)
            }
            InstallationSource::LocalRepo { path } => local::materialize(path),
        }
        .map_err(Into::into)
    }

    fn shipped_dll_is_current(
        &self,
        source: &InstallationSource,
        dll_path: &str,
        progress: &ProgressReporter,
    ) -> Result<bool, BoundaryError> {
        match source {
            InstallationSource::UpstreamCache { version } => self
                .upstream
                .shipped_dll_is_current(version, dll_path, progress)
                .map_err(Into::into),
            // A Local Repo is the working tree, uncommitted changes and all, so no commit
            // can vouch for the DLL sitting in it - see the trait's default.
            InstallationSource::LocalRepo { .. } => Ok(false),
        }
    }

    fn available_versions(
        &self,
        progress: &ProgressReporter,
    ) -> Result<civ5vp_core::VersionCatalog, BoundaryError> {
        self.upstream.list_versions(progress).map_err(Into::into)
    }

    fn unofficial_versions(
        &self,
        releases: &[String],
        progress: &ProgressReporter,
    ) -> Result<Vec<civ5vp_core::UnofficialVersion>, BoundaryError> {
        self.upstream
            .list_unofficial(releases, progress)
            .map_err(Into::into)
    }

    fn materialize_luajit(&self, progress: &ProgressReporter) -> Result<PathBuf, BoundaryError> {
        self.luajit.materialize(progress)
    }
}
