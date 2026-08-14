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

/// Which of the two proven compiler configurations the Built DLL is compiled with.
///
/// Players always get [`BuildConfiguration::Release`]; the Debug choice arrives with Dev mode
/// (ticket 08, user story 31). It lives here rather than in the toolchain crate because the
/// Core decides it and hands it across the boundary in a [`crate::BuildRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildConfiguration {
    Release,
    Debug,
}

impl BuildConfiguration {
    /// The one lowercase token used everywhere this is written down — the settings file and
    /// the Build Fingerprint — so the two can never drift apart.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Debug => "debug",
        }
    }
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

impl InstallationSource {
    /// A short name for what is being built: the Release tag, the branch or ref, or `Local`
    /// for a developer checkout.
    ///
    /// The DLL sources compile this into the binary as their version string (upstream
    /// generates it with `git describe`; the installer runs no git, so the selected Version is
    /// the honest equivalent). It reaches the toolchain runner via [`crate::BuildRequest`].
    pub fn version_label(&self) -> String {
        match self {
            Self::UpstreamCache { version } => match version {
                Version::Release(tag) => tag.clone(),
                Version::LatestDevelopmentVersion => "master".to_owned(),
                Version::ArbitraryRef(reference) => reference.clone(),
            },
            Self::LocalRepo { .. } => "Local".to_owned(),
        }
    }

    /// The Installation Source of a player who has not named one yet.
    ///
    /// A Local Repo with no path. The Core refuses it with a sentence saying so, which is the
    /// right answer — "you have not said where the sources come from" is a thing to be told,
    /// not a state to be represented separately.
    pub fn unchosen() -> Self {
        Self::LocalRepo {
            path: PathBuf::new(),
        }
    }
}

impl Flavor {
    /// What to offer a player who has never run the installer.
    ///
    /// Vox Populi with EUI. Vox Populi is the mod people mean when they say "Vox Populi", and
    /// EUI is part of the standard experience — Community Patch alone is the smaller, more
    /// deliberate choice. Deciding this is the Core's job, not the shell's (rule 3).
    pub fn suggested() -> Self {
        Self::VoxPopuli { eui: Eui::Enabled }
    }
}

/// The complete user selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallConfiguration {
    pub source: InstallationSource,
    pub flavor: Flavor,
    pub forty_three_civs: FortyThreeCivs,
    /// Release for players; Debug is a Dev-mode choice (user story 31) and is only legal
    /// with a Local Repo — [`crate::Core::plan`] refuses it anywhere else.
    pub build_configuration: BuildConfiguration,
}
