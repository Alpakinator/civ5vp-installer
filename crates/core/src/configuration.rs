//! The user's selection: Installation Source + Version + Flavor + toggles.

use std::path::PathBuf;

/// The base choice of what to install.
///
/// EUI lives *inside* [`Flavor::VoxPopuli`] rather than beside it, so "EUI with Community
/// Patch only" — the one illegal combination — cannot be written down at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flavor {
    /// `(1) Community Patch` alone.
    CommunityPatch,
    /// `(1)` + `(2)` + Squads + VPUI, optionally with EUI.
    VoxPopuli { eui: Eui },
}

/// The Enhanced User Interface toggle. Only reachable through [`Flavor::VoxPopuli`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eui {
    Enabled,
    Disabled,
}

/// Whether the Built DLL is compiled with the 43-civ setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FortyThreeCivs {
    Enabled,
    Disabled,
}

/// The ref of the Community-Patch-DLL repository being installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// A `Release-*` tag.
    Release(String),
    /// Upstream `master` HEAD.
    LatestDevelopmentVersion,
    /// Any branch, tag, or commit. Advanced users only.
    ArbitraryRef(String),
}

/// Where the mod files and DLL sources come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallationSource {
    /// The installer-managed clone in the App Data Store, checked out at `version`.
    UpstreamCache { version: Version },
    /// A developer's own checkout, used as-is including uncommitted changes.
    LocalRepo { path: PathBuf },
}

/// The complete user selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallConfiguration {
    pub source: InstallationSource,
    pub flavor: Flavor,
    pub forty_three_civs: FortyThreeCivs,
}
