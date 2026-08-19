//! The Core's one platform adapter.
//!
//! This is the only place in the crate where `#[cfg(windows)]` or `#[cfg(unix)]` appears, and
//! it is deliberately the thinnest thing that could work: it produces *candidate directories*
//! and answers "where does this platform keep app data". It decides nothing. Every candidate
//! it hands back goes through exactly the same validation as a folder the user typed in, and
//! that validation is platform-agnostic and covered by `tests/detection.rs` on Linux.
//!
//! **Windows is not verified here.** The spec's platform-verification constraint says so
//! plainly: there is no Windows machine and no CI runner yet. What the Windows half does is
//! therefore restricted to reading the environment variables every Windows process has -
//! honest, and wrong only in ways a user can correct by picking the folders by hand. The
//! known-folder API (`SHGetKnownFolderPath`) and the Steam registry key
//! (`HKCU\Software\Valve\Steam\SteamPath`) are the two things that would make it exact; both
//! need `unsafe` or a `windows` crate dependency plus a Windows runner to verify, and both
//! are follow-up work rather than something to fake here.

use std::path::PathBuf;

use super::SearchLocations;

/// The directory name of the App Data Store inside the platform's app-data location.
#[cfg(unix)]
const APP_DATA_FOLDER_NAME: &str = "civ5vp-installer";
#[cfg(windows)]
const APP_DATA_FOLDER_NAME: &str = "Civ 5 VP Installer";

/// Which environment variable was missing, when the app-data location cannot be worked out.
#[cfg(unix)]
pub(crate) const APP_DATA_VARIABLE: &str = "HOME";
#[cfg(windows)]
pub(crate) const APP_DATA_VARIABLE: &str = "LOCALAPPDATA";

fn variable(name: &str) -> Option<PathBuf> {
    match std::env::var_os(name) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

/// Where the App Data Store lives: `%LOCALAPPDATA%` on Windows, the XDG data directory on
/// Linux.
#[cfg(unix)]
pub(crate) fn app_data_root() -> Option<PathBuf> {
    let data_home = variable("XDG_DATA_HOME").or_else(|| variable("HOME").map(home_data_dir))?;
    Some(data_home.join(APP_DATA_FOLDER_NAME))
}

#[cfg(unix)]
fn home_data_dir(home: PathBuf) -> PathBuf {
    home.join(".local").join("share")
}

#[cfg(windows)]
pub(crate) fn app_data_root() -> Option<PathBuf> {
    Some(variable("LOCALAPPDATA")?.join(APP_DATA_FOLDER_NAME))
}

/// Where this platform keeps Steam, and where it keeps the user's Documents.
#[cfg(unix)]
pub(crate) fn search_locations() -> SearchLocations {
    let mut steam_roots = Vec::new();

    if let Some(data_home) = variable("XDG_DATA_HOME") {
        steam_roots.push(data_home.join("Steam"));
    }
    if let Some(home) = variable("HOME") {
        steam_roots.push(home_data_dir(home.clone()).join("Steam"));
        // Both are symlinks to the real library on a normal install, but which one exists
        // varies with how old the installation is.
        steam_roots.push(home.join(".steam").join("steam"));
        steam_roots.push(home.join(".steam").join("root"));
        // The Flatpak build keeps its own home.
        steam_roots.push(
            home.join(".var/app/com.valvesoftware.Steam/.local/share")
                .join("Steam"),
        );
    }

    SearchLocations {
        steam_roots,
        // On Linux the Documents side is inside a Proton prefix, which detection derives from
        // the Steam libraries. There is no separate Documents location to offer.
        documents_roots: Vec::new(),
    }
}

#[cfg(windows)]
pub(crate) fn search_locations() -> SearchLocations {
    let mut steam_roots = Vec::new();
    // The registry (`HKCU\Software\Valve\Steam\SteamPath`) is the exact answer; these are the
    // default install locations, which is where Steam is unless the user moved it. A user who
    // moved it picks the folder by hand until the registry lookup lands.
    for program_files in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(root) = variable(program_files) {
            steam_roots.push(root.join("Steam"));
        }
    }
    if let Some(profile) = variable("USERPROFILE") {
        steam_roots.push(profile.join("scoop/apps/steam/current"));
    }

    let mut documents_roots = Vec::new();
    if let Some(profile) = variable("USERPROFILE") {
        // `SHGetKnownFolderPath(FOLDERID_Documents)` is the exact answer, and it is the one
        // that copes with a redirected Documents folder. These two cover the default and the
        // most common redirection.
        documents_roots.push(profile.join("Documents"));
        documents_roots.push(profile.join("OneDrive").join("Documents"));
    }

    SearchLocations {
        steam_roots,
        documents_roots,
    }
}
