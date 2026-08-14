//! The App Data Store, what the installer remembers in it, and what it knows at launch.
//!
//! The store is the single installer-owned directory in the platform's app-data location. The
//! Upstream Cache, the Toolchain Cache and the log file join the settings here in later
//! tickets; today it holds the settings file and the Core's work directory.
//!
//! The file format is hand-rolled `key = value` lines. That is a deliberate choice, made for
//! the same reason as the VDF parser and the CLI parser: the Core has no dependencies, and a
//! settings file a player can open and read is worth more here than a schema.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::configuration::{
    BuildConfiguration, Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallationSource,
    Version,
};
use crate::detect::{self, Detection, FolderRejected, SearchLocations};

/// The settings file's name inside the App Data Store.
const SETTINGS_FILE_NAME: &str = "settings.txt";

const HEADER: &str = "\
# Civ 5 VP Installer — remembered settings.
# Written by the installer whenever something worth remembering changes.
# Lines it does not understand are ignored, so editing this by hand is safe enough.
";

/// Something that stopped the installer remembering, or reading what it remembered.
#[derive(Debug)]
pub enum SettingsError {
    /// The platform did not say where app data goes.
    NoAppDataLocation { variable: &'static str },
    /// A file operation against the App Data Store failed.
    Io {
        /// What was being attempted, e.g. "read" — used to build the log line.
        action: &'static str,
        path: PathBuf,
        cause: io::Error,
    },
}

impl SettingsError {
    /// The sentence shown in the UI (rule 10).
    pub fn user_message(&self) -> String {
        match self {
            Self::NoAppDataLocation { .. } => "Could not work out where to keep the installer's \
                 own files. Your settings will not be remembered between runs."
                .to_owned(),
            Self::Io { path, .. } => format!(
                "Could not use {}, so your settings will not be remembered. Check that the \
                 folder exists and is not read-only.",
                path.display()
            ),
        }
    }

    /// The full detail, for the log file (rule 11).
    pub fn log_detail(&self) -> String {
        match self {
            Self::NoAppDataLocation { variable } => {
                format!("app data store: {variable} is not set")
            }
            Self::Io {
                action,
                path,
                cause,
            } => format!(
                "app data store: {action} {} failed: {cause}",
                path.display()
            ),
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { cause, .. } => Some(cause),
            Self::NoAppDataLocation { .. } => None,
        }
    }
}

/// The installer-owned directory in the platform's app-data location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDataStore {
    root: PathBuf,
}

impl AppDataStore {
    /// The real one: `%LOCALAPPDATA%\Civ 5 VP Installer` on Windows, the XDG data directory on
    /// Linux. The directory is not created until something is written to it.
    pub fn for_this_platform() -> Result<Self, SettingsError> {
        match detect::app_data_root() {
            Some(root) => Ok(Self { root }),
            None => Err(SettingsError::NoAppDataLocation {
                variable: detect::APP_DATA_VARIABLE,
            }),
        }
    }

    /// A store wherever the caller says. How the tests get one that is not the user's.
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE_NAME)
    }

    /// Read what was remembered. A store that has never been written to is a first run, not a
    /// failure, and neither is a line this version does not understand.
    pub fn load(&self) -> Result<Settings, SettingsError> {
        let path = self.settings_file();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(cause) => {
                return Err(SettingsError::Io {
                    action: "read",
                    path,
                    cause,
                });
            }
        };
        Ok(Settings::parse(&text))
    }

    /// Write what to remember, creating the store if it is not there yet.
    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        fs::create_dir_all(&self.root).map_err(|cause| SettingsError::Io {
            action: "create",
            path: self.root.clone(),
            cause,
        })?;
        let path = self.settings_file();
        fs::write(&path, settings.to_text()).map_err(|cause| SettingsError::Io {
            action: "write",
            path,
            cause,
        })
    }
}

/// What the installer remembers between runs (user story 26).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// The Game Installation the player last used.
    pub game_installation: Option<PathBuf>,
    /// The Documents side that went with it.
    pub documents_folder: Option<PathBuf>,
    /// The last Install Configuration.
    pub configuration: Option<InstallConfiguration>,
}

impl Settings {
    fn to_text(&self) -> String {
        let mut text = String::from(HEADER);
        write_path(
            &mut text,
            "game-installation",
            self.game_installation.as_ref(),
        );
        write_path(
            &mut text,
            "documents-folder",
            self.documents_folder.as_ref(),
        );

        let Some(configuration) = &self.configuration else {
            return text;
        };
        match &configuration.source {
            // A Local Repo with no path is "not chosen yet", not a choice. Writing it would
            // produce a `local-repo = ` line that reads back as nothing useful; leaving the
            // source out entirely says the same thing and reads back the same way.
            InstallationSource::LocalRepo { path } if path.as_os_str().is_empty() => {}
            InstallationSource::LocalRepo { path } => {
                write_line(&mut text, "source", "local-repo");
                write_path(&mut text, "local-repo", Some(path));
            }
            InstallationSource::UpstreamCache { version } => {
                write_line(&mut text, "source", "upstream-cache");
                write_line(&mut text, "version", &version_value(version));
            }
        }
        match &configuration.flavor {
            Flavor::CommunityPatch => write_line(&mut text, "flavor", "community-patch"),
            Flavor::VoxPopuli { eui } => {
                write_line(&mut text, "flavor", "vox-populi");
                write_line(&mut text, "eui", on_off(*eui == Eui::Enabled));
            }
        }
        write_line(
            &mut text,
            "forty-three-civs",
            on_off(configuration.forty_three_civs == FortyThreeCivs::Enabled),
        );
        write_line(
            &mut text,
            "build-configuration",
            configuration.build_configuration.token(),
        );
        text
    }

    fn parse(text: &str) -> Self {
        let values = Values::parse(text);
        Self {
            game_installation: values.path("game-installation"),
            documents_folder: values.path("documents-folder"),
            configuration: read_configuration(&values),
        }
    }
}

/// Everything the installer knows at launch: what it remembered, reconciled with what it can
/// find (user story 11 and 26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Startup {
    /// What to put in the game folder field, if anything.
    pub game_installation: Option<PathBuf>,
    /// What to put in the Documents folder field, if anything.
    pub documents_folder: Option<PathBuf>,
    /// The Install Configuration to start from, if one was remembered.
    pub configuration: Option<InstallConfiguration>,
    /// What to tell the player, when the folders could not be settled. Already plain language.
    pub note: Option<String>,
    /// Lines for the log file (rule 11).
    pub log: Vec<String>,
}

impl Startup {
    /// The sentence to show when the game folders could not be settled.
    ///
    /// Two explanations can apply at once, and the more specific one wins. `note` is about this
    /// machine — that the game found is the native Aspyr port, or that nothing was found at all
    /// — while `rejected` is about one field being wrong. "This is the Linux port and Vox Populi
    /// cannot use it" tells a player far more than "choose your Documents folder".
    pub fn explanation(&self, rejected: &FolderRejected) -> String {
        self.note.clone().unwrap_or_else(|| rejected.user_message())
    }
}

/// Work out what the installer knows at launch.
///
/// What was remembered wins, as long as both folders are still the real thing: a player who
/// picked their folders by hand does not want them quietly replaced by a guess. Otherwise
/// detection has the last word, and when that finds nothing usable its explanation becomes
/// the note the shell shows.
pub fn start_up(store: &AppDataStore, locations: &SearchLocations) -> Startup {
    let mut log = Vec::new();

    let settings = match store.load() {
        Ok(settings) => settings,
        Err(err) => {
            log.push(err.log_detail());
            Settings::default()
        }
    };

    let mut remembered_problem = None;
    if let (Some(game), Some(documents)) = (&settings.game_installation, &settings.documents_folder)
    {
        match detect::resolve_game_folders(game, documents) {
            Ok(_) => {
                log.push(format!(
                    "startup: using the remembered folders {} and {}",
                    game.display(),
                    documents.display()
                ));
                return Startup {
                    game_installation: Some(game.clone()),
                    documents_folder: Some(documents.clone()),
                    configuration: settings.configuration,
                    note: None,
                    log,
                };
            }
            Err(rejected) => {
                log.push(format!("startup: {}", rejected.log_detail()));
                remembered_problem = Some(rejected);
            }
        }
    }

    let detection = detect::detect_game(locations);
    log.push(detection.log_detail());

    match detection {
        Detection::Found(game) => Startup {
            game_installation: Some(game.game_installation.root().to_path_buf()),
            documents_folder: Some(game.documents.root().to_path_buf()),
            configuration: settings.configuration,
            note: None,
            log,
        },
        Detection::DocumentsNotFound {
            ref game_installation,
            ..
        } => Startup {
            game_installation: Some(game_installation.root().to_path_buf()),
            documents_folder: settings.documents_folder,
            configuration: settings.configuration,
            note: detection.user_message(),
            log,
        },
        Detection::Refused(_) | Detection::NotFound { .. } => Startup {
            // Whatever was remembered is shown even though it did not check out, so that a
            // player who moved one folder can correct it instead of retyping both.
            game_installation: settings.game_installation,
            documents_folder: settings.documents_folder,
            configuration: settings.configuration,
            // A remembered folder that stopped working is the more useful thing to explain:
            // it is about this player's machine, not about a search that came up empty.
            note: remembered_problem
                .map(|rejected| rejected.user_message())
                .or_else(|| detection.user_message()),
            log,
        },
    }
}

fn version_value(version: &Version) -> String {
    match version {
        Version::Release(tag) => format!("release:{tag}"),
        Version::LatestDevelopmentVersion => "latest-development".to_owned(),
        Version::ArbitraryRef(reference) => format!("ref:{reference}"),
    }
}

fn read_version(value: &str) -> Option<Version> {
    if value == "latest-development" {
        return Some(Version::LatestDevelopmentVersion);
    }
    match value.split_once(':') {
        Some(("release", tag)) if !tag.is_empty() => Some(Version::Release(tag.to_owned())),
        Some(("ref", reference)) if !reference.is_empty() => {
            Some(Version::ArbitraryRef(reference.to_owned()))
        }
        _ => None,
    }
}

/// An Install Configuration is remembered whole or not at all: half of one restored from a
/// file written by a different version would be a configuration the player never chose.
///
/// The Installation Source is the exception, and deliberately. A player who has not yet said
/// where the sources come from has still said which Flavor they want, and dropping the whole
/// configuration for the sake of the one part they have not filled in would mean their Flavor
/// silently never persisted. An unset source reads back as a Local Repo with no path — which
/// is exactly what it is, and which the Core already refuses with a sentence saying so.
fn read_configuration(values: &Values) -> Option<InstallConfiguration> {
    let source = match values.get("source") {
        Some("local-repo") => InstallationSource::LocalRepo {
            path: values.path("local-repo").unwrap_or_default(),
        },
        Some("upstream-cache") => InstallationSource::UpstreamCache {
            version: read_version(values.get("version")?)?,
        },
        None => InstallationSource::LocalRepo {
            path: PathBuf::new(),
        },
        Some(_) => return None,
    };
    let flavor = match values.get("flavor")? {
        "community-patch" => Flavor::CommunityPatch,
        "vox-populi" => Flavor::VoxPopuli {
            eui: if values.get("eui") == Some("on") {
                Eui::Enabled
            } else {
                Eui::Disabled
            },
        },
        _ => return None,
    };
    let forty_three_civs = if values.get("forty-three-civs") == Some("on") {
        FortyThreeCivs::Enabled
    } else {
        FortyThreeCivs::Disabled
    };
    // Anything but an explicit "debug" — including a file from before the line existed —
    // reads as Release, the configuration every player gets.
    let build_configuration = if values.get("build-configuration") == Some("debug") {
        BuildConfiguration::Debug
    } else {
        BuildConfiguration::Release
    };
    Some(InstallConfiguration {
        source,
        flavor,
        forty_three_civs,
        build_configuration,
    })
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn write_line(text: &mut String, key: &str, value: &str) {
    text.push_str(key);
    text.push_str(" = ");
    text.push_str(value);
    text.push('\n');
}

/// A path is written only if it is text. A path that is not valid UTF-8 is simply not
/// remembered — the file stays readable, and the worst that happens is one folder to pick
/// again. Nor is one containing a line break, which would be read back as two settings.
fn write_path(text: &mut String, key: &str, path: Option<&PathBuf>) {
    let Some(value) = path.and_then(|path| path.to_str()) else {
        return;
    };
    if value.contains(['\n', '\r']) || value.trim() != value {
        return;
    }
    write_line(text, key, value);
}

/// The `key = value` lines of a settings file, in the order they were read. Anything else in
/// the file — comments, blank lines, keys from another version — is dropped here.
struct Values {
    pairs: Vec<(String, String)>,
}

impl Values {
    fn parse(text: &str) -> Self {
        let mut pairs = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            pairs.push((key.trim().to_owned(), value.trim().to_owned()));
        }
        Self { pairs }
    }

    /// The last value given for `key`, so a file that somehow names one twice reads the way it
    /// would have been written.
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        self.get(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}
