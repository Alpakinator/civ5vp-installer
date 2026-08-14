//! The real [`SourceProvider`] — boundary one of the Core's two (rule 2).

use civ5vp_core::{
    BoundaryError, InstallationSource, MaterializedSource, ProgressReporter, SourceProvider,
};

use crate::local;
use crate::upstream::UpstreamCache;

/// Both Installation Sources, because the Core sees them as one boundary.
///
/// The Upstream Cache is a managed clone that has to be fetched and checked out; a Local Repo
/// is handed straight back. Which of the two applies is decided by the
/// [`InstallationSource`] the Core passes in, not by configuration here.
pub struct InstallationSources {
    upstream: UpstreamCache,
}

impl InstallationSources {
    pub fn new(upstream: UpstreamCache) -> Self {
        Self { upstream }
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

    fn available_versions(
        &self,
        progress: &ProgressReporter,
    ) -> Result<civ5vp_core::VersionCatalog, BoundaryError> {
        self.upstream.list_versions(progress).map_err(Into::into)
    }

    fn unofficial_versions(
        &self,
        newest_release: &str,
        progress: &ProgressReporter,
    ) -> Result<Vec<civ5vp_core::UnofficialVersion>, BoundaryError> {
        self.upstream
            .list_unofficial(newest_release, progress)
            .map_err(Into::into)
    }
}
