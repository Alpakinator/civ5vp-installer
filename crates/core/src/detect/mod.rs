//! Finding the game - and deciding whether a folder really is the game.
//!
//! Two halves, deliberately separated:
//!
//! * this module, which is platform-agnostic. Given directories to look in, it judges what it
//!   finds against the markers below and resolves the three Deployment targets. Every decision
//!   lives here, and every one of them is exercised on Linux against fixture trees.
//! * [`platform`], the only `#[cfg]`-split code in the crate. It produces candidate
//!   directories and nothing else.
//!
//! The same judging is used by detection and by the manual picker, so a folder Steam led us to
//! and a folder the user typed in are held to exactly the same standard.

use std::fs;
use std::path::{Path, PathBuf};

use crate::claimed::GameFolders;

mod platform;
mod vdf;

// The App Data Store's location is the other question only the platform can answer, so it is
// answered in the same adapter rather than in a second `#[cfg]` split elsewhere.
pub(crate) use platform::{APP_DATA_VARIABLE, app_data_root};

/// The user's home directory, as this platform reports it. The shell hands it back to
/// [`browse_start`] as the ladder's last rung.
pub fn home_directory() -> Option<PathBuf> {
    platform::home_directory()
}

/// Steam's app id for Civilization V. It names the Proton prefix on Linux:
/// `steamapps/compatdata/8930`.
const STEAM_APP_ID: &str = "8930";

/// The Game Installation's folder name inside a Steam library. Roman numeral.
const GAME_FOLDER_NAME: &str = "Sid Meier's Civilization V";

/// The Documents side's folder name. Arabic numeral - a *different name*, not a different
/// spelling of the same one. Neither is ever derived from the other: substituting the numeral
/// produces a folder that does not exist, and `tests/detection.rs` plants a decoy at exactly
/// that path to make the mistake fail loudly rather than silently.
const DOCUMENTS_FOLDER_NAME: &str = "Sid Meier's Civilization 5";

/// What must be present for a folder to be the Steam install of the *Windows* game. Paths are
/// relative to the Game Installation root, `/`-separated.
const GAME_MARKERS: [&str; 3] = ["CivilizationV.exe", "CivilizationV_DX11.exe", "Assets/DLC"];

/// Brave New World. Vox Populi requires it, so a game without it is refused rather than
/// warned about.
const BRAVE_NEW_WORLD_MARKER: &str = "Assets/DLC/Expansion2";

/// What must be present for a folder to be the game's Documents side.
const DOCUMENTS_MARKERS: [&str; 4] = ["MODS", "Text", "ModUserData", "UserSettings.ini"];

/// What the native Aspyr Linux port has where the Windows version has its executables. Only
/// used to tell a player *which* Civilization V they have; it is never a thing to install to.
const NATIVE_PORT_MARKERS: [&str; 2] = ["Civ5XP", "steamassets"];

/// The DLC Folder is this, under the Game Installation. It is not configurable.
const DLC_SUBDIR: &str = "Assets/DLC";

/// Which of the two folders a judgement was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    /// The Game Installation: `…/steamapps/common/Sid Meier's Civilization V`.
    GameInstallation,
    /// The Documents side: `…/My Games/Sid Meier's Civilization 5`.
    Documents,
}

impl FolderKind {
    /// How the folder is named to the user. The numeral is part of the name and is quoted
    /// back at the player, because telling the two folders apart is the whole difficulty.
    fn label(self) -> &'static str {
        match self {
            Self::GameInstallation => "Civilization V game folder",
            Self::Documents => "Civilization 5 Documents folder",
        }
    }
}

/// Why a folder is not the thing it was supposed to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// No path at all.
    NotChosen,
    /// A relative path, which would be resolved against the working directory.
    NotAbsolute,
    /// Nothing there, or there but not a directory.
    NotADirectory,
    /// A marker is missing. Named, so the message can say what was looked for.
    MissingMarker { marker: &'static str },
    /// The native Aspyr Linux port, which cannot load the Built DLL.
    NativeLinuxPort,
    /// Civilization V, without the Brave New World expansion.
    BraveNewWorldMissing,
}

/// A folder that was checked and found not to be what it needed to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRejected {
    pub folder: FolderKind,
    pub path: PathBuf,
    pub reason: RejectionReason,
}

impl FolderRejected {
    /// The marker that was missing, when that is why this folder was rejected.
    pub fn missing_marker(&self) -> Option<&'static str> {
        match self.reason {
            RejectionReason::MissingMarker { marker } => Some(marker),
            RejectionReason::BraveNewWorldMissing => Some(BRAVE_NEW_WORLD_MARKER),
            _ => None,
        }
    }

    /// The sentence shown in the UI.
    pub fn user_message(&self) -> String {
        let label = self.folder.label();
        match self.reason {
            RejectionReason::NotChosen => format!("Choose your {label} before installing."),
            RejectionReason::NotAbsolute => format!(
                "The {label} needs to be a full path starting from the root of the drive, not \
                 \"{}\".",
                self.path.display()
            ),
            RejectionReason::NotADirectory => format!(
                "There is no folder at {}. Check the path and try again.",
                self.path.display()
            ),
            RejectionReason::MissingMarker { marker } => format!(
                "{} is not the {label}: it has no {marker} in it. Pick the folder that does.",
                self.path.display()
            ),
            RejectionReason::NativeLinuxPort => format!(
                "{} is the native Linux version of Civilization V from Aspyr. Vox Populi \
                 needs the Windows version running under Proton and cannot be installed into \
                 the native port. In Steam, open the game's properties, set a Proton \
                 compatibility tool, run the game once, then try again.",
                self.path.display()
            ),
            RejectionReason::BraveNewWorldMissing => format!(
                "The Civilization V at {} does not have the Brave New World expansion, which \
                 Vox Populi needs. Install Brave New World and try again.",
                self.path.display()
            ),
        }
    }

    /// The full detail, for the log file.
    pub fn log_detail(&self) -> String {
        format!(
            "{:?} rejected at {}: {:?}",
            self.folder,
            self.path.display(),
            self.reason
        )
    }
}

/// A folder that has been checked against the markers and really is the Steam install of the
/// Windows game, with Brave New World.
///
/// Only [`validate_game_installation`] makes one, so holding one is the proof that the check
/// happened - a caller cannot assemble an unchecked one and hand it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInstallation {
    root: PathBuf,
    dlc_folder: PathBuf,
}

impl GameInstallation {
    /// `…/Sid Meier's Civilization V`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The DLC Folder: `<game>/Assets/DLC`.
    pub fn dlc_folder(&self) -> &Path {
        &self.dlc_folder
    }
}

/// A folder that has been checked against the markers and really is the game's Documents side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentsFolder {
    root: PathBuf,
    mods_folder: PathBuf,
    text_folder: PathBuf,
}

impl DocumentsFolder {
    /// `…/My Games/Sid Meier's Civilization 5`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The MODS Folder.
    pub fn mods_folder(&self) -> &Path {
        &self.mods_folder
    }

    /// The Text Folder.
    pub fn text_folder(&self) -> &Path {
        &self.text_folder
    }
}

/// The three Deployment targets these two folders imply: MODS and Text from the Documents
/// side, DLC from the Game Installation. Each comes from the folder it actually lives in.
pub fn game_folders(game: &GameInstallation, documents: &DocumentsFolder) -> GameFolders {
    GameFolders {
        mods: documents.mods_folder().to_path_buf(),
        dlc: game.dlc_folder().to_path_buf(),
        text: documents.text_folder().to_path_buf(),
        game_root: game.root().to_path_buf(),
    }
}

/// The manual picker's whole job: check both folders the user chose and resolve the three
/// Deployment targets, or say which folder is wrong and why.
pub fn resolve_game_folders(game: &Path, documents: &Path) -> Result<GameFolders, FolderRejected> {
    let game = validate_game_installation(game)?;
    let documents = validate_documents_folder(documents)?;
    Ok(game_folders(&game, &documents))
}

/// Is this folder the Steam install of the Windows game?
pub fn validate_game_installation(path: &Path) -> Result<GameInstallation, FolderRejected> {
    let kind = FolderKind::GameInstallation;
    let root = usable_directory(path, kind)?;

    // The native port first. It really is Civilization V - it just cannot load the Built DLL -
    // and saying so is far more use to the player than "CivilizationV.exe is missing".
    if locate(&root, GAME_MARKERS[0]).is_none()
        && NATIVE_PORT_MARKERS
            .iter()
            .any(|marker| locate(&root, marker).is_some())
    {
        return Err(reject(kind, root, RejectionReason::NativeLinuxPort));
    }

    for marker in GAME_MARKERS {
        if locate(&root, marker).is_none() {
            return Err(reject(
                kind,
                root,
                RejectionReason::MissingMarker { marker },
            ));
        }
    }
    if locate(&root, BRAVE_NEW_WORLD_MARKER).is_none() {
        return Err(reject(kind, root, RejectionReason::BraveNewWorldMissing));
    }

    let Some(dlc_folder) = locate(&root, DLC_SUBDIR) else {
        return Err(reject(
            kind,
            root,
            RejectionReason::MissingMarker { marker: DLC_SUBDIR },
        ));
    };
    Ok(GameInstallation { root, dlc_folder })
}

/// Is this folder the game's Documents side?
pub fn validate_documents_folder(path: &Path) -> Result<DocumentsFolder, FolderRejected> {
    let kind = FolderKind::Documents;
    let root = usable_directory(path, kind)?;

    for marker in DOCUMENTS_MARKERS {
        if locate(&root, marker).is_none() {
            return Err(reject(
                kind,
                root,
                RejectionReason::MissingMarker { marker },
            ));
        }
    }

    let (Some(mods_folder), Some(text_folder)) = (locate(&root, "MODS"), locate(&root, "Text"))
    else {
        return Err(reject(
            kind,
            root,
            RejectionReason::MissingMarker { marker: "MODS" },
        ));
    };
    Ok(DocumentsFolder {
        root,
        mods_folder,
        text_folder,
    })
}

fn reject(folder: FolderKind, path: PathBuf, reason: RejectionReason) -> FolderRejected {
    FolderRejected {
        folder,
        path,
        reason,
    }
}

/// The floor every picked path has to clear before its contents are worth looking at: a real,
/// absolute directory - Sync derives its write paths from these roots.
fn usable_directory(path: &Path, folder: FolderKind) -> Result<PathBuf, FolderRejected> {
    let reason = if path.as_os_str().is_empty() {
        RejectionReason::NotChosen
    } else if !path.is_absolute() {
        RejectionReason::NotAbsolute
    } else if !path.is_dir() {
        RejectionReason::NotADirectory
    } else {
        return Ok(path.to_path_buf());
    };
    Err(reject(folder, path.to_path_buf(), reason))
}

/// Where detection is allowed to look.
///
/// The platform adapter fills this in for the machine the installer is running on
/// ([`SearchLocations::for_this_platform`]); the tests fill it in with fixture directories,
/// which is how the Windows arrangement is exercised on Linux.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchLocations {
    /// Steam installation roots - the folders that hold `steamapps/`. Every library named in
    /// each one's `libraryfolders.vdf` is searched too.
    pub steam_roots: Vec<PathBuf>,
    /// Documents folders to look under, for `My Games/Sid Meier's Civilization 5`. This is the
    /// Windows arrangement; on Linux the Documents side lives in a Proton prefix instead and
    /// this is empty.
    pub documents_roots: Vec<PathBuf>,
}

impl SearchLocations {
    /// What this platform says to look at. The one platform-specific step in detection.
    pub fn for_this_platform() -> Self {
        platform::search_locations()
    }
}

/// A Game Installation and the Documents side that goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedGame {
    pub game_installation: GameInstallation,
    pub documents: DocumentsFolder,
}

impl DetectedGame {
    /// The three Deployment targets.
    pub fn folders(&self) -> GameFolders {
        game_folders(&self.game_installation, &self.documents)
    }
}

/// What detection concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// Everything needed, found.
    Found(DetectedGame),
    /// The game is there but its Documents side is not - which is what a game that has never
    /// been launched looks like.
    DocumentsNotFound {
        game_installation: GameInstallation,
        /// Where the Documents side was looked for, for the log.
        searched: Vec<PathBuf>,
    },
    /// Civilization V is there and cannot be used: the native Aspyr port, or a game without
    /// Brave New World.
    Refused(FolderRejected),
    /// Nothing that looks like Civilization V turned up anywhere.
    NotFound {
        /// Where we looked, for the log.
        searched: Vec<PathBuf>,
    },
}

impl Detection {
    /// What to tell the player, when detection did not produce a usable answer. `None` means
    /// there is nothing to explain.
    pub fn user_message(&self) -> Option<String> {
        match self {
            Self::Found(_) => None,
            Self::DocumentsNotFound {
                game_installation, ..
            } => Some(format!(
                "Found Civilization V at {}, but not the folder the game keeps your mods and \
                 settings in. Start the game once so that it creates that folder, then reopen \
                 this installer - or enter the folder below yourself.",
                game_installation.root().display()
            )),
            Self::Refused(rejected) => Some(rejected.user_message()),
            Self::NotFound { .. } => Some(
                "Could not find Civilization V automatically. Enter the game folder and the \
                 Documents folder below."
                    .to_owned(),
            ),
        }
    }

    /// The full detail, for the log file.
    pub fn log_detail(&self) -> String {
        match self {
            Self::Found(game) => format!(
                "detection: game at {}, documents at {}",
                game.game_installation.root().display(),
                game.documents.root().display()
            ),
            Self::DocumentsNotFound {
                game_installation,
                searched,
            } => format!(
                "detection: game at {}, no documents folder in {}",
                game_installation.root().display(),
                list(searched)
            ),
            Self::Refused(rejected) => format!("detection: {}", rejected.log_detail()),
            Self::NotFound { searched } => {
                format!("detection: nothing found in {}", list(searched))
            }
        }
    }
}

fn list(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "no candidate locations".to_owned();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Look for the game in the given locations.
///
/// The first candidate that is a complete Game Installation wins; a candidate that is
/// Civilization V but unusable is remembered so that its explanation can be given if nothing
/// better turns up. Candidates are visited in the order the locations name them, so the same
/// machine always produces the same answer.
pub fn detect_game(locations: &SearchLocations) -> Detection {
    let libraries = steam_libraries(&locations.steam_roots);

    let mut searched = Vec::new();
    let mut refusal: Option<FolderRejected> = None;
    let mut installation = None;

    for library in &libraries {
        let relative = format!("steamapps/common/{GAME_FOLDER_NAME}");
        let Some(candidate) = locate(library, &relative) else {
            searched.push(library.join(&relative));
            continue;
        };
        match validate_game_installation(&candidate) {
            Ok(found) => {
                installation = Some(found);
                break;
            }
            Err(rejected) => {
                searched.push(candidate);
                refusal = refusal.or(Some(rejected));
            }
        }
    }

    let mut documents_searched = Vec::new();
    let mut documents = None;
    for (root, relative) in documents_candidates(&libraries, &locations.documents_roots) {
        let Some(candidate) = locate(&root, &relative) else {
            documents_searched.push(root.join(&relative));
            continue;
        };
        match validate_documents_folder(&candidate) {
            Ok(found) => {
                documents = Some(found);
                break;
            }
            Err(_) => documents_searched.push(candidate),
        }
    }

    match (installation, documents) {
        (Some(game_installation), Some(documents)) => Detection::Found(DetectedGame {
            game_installation,
            documents,
        }),
        (Some(game_installation), None) => Detection::DocumentsNotFound {
            game_installation,
            searched: documents_searched,
        },
        (None, _) => match refusal {
            Some(rejected) => Detection::Refused(rejected),
            None => {
                searched.extend(documents_searched);
                Detection::NotFound { searched }
            }
        },
    }
}

/// Every Steam library: the given roots, plus everything their `libraryfolders.vdf` names.
fn steam_libraries(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    for root in steam_roots {
        push_unique(&mut libraries, root.clone());
        // Steam has kept this file in both places over the years.
        for relative in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
            let Some(file) = locate(root, relative) else {
                continue;
            };
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for path in vdf::library_paths(&text) {
                push_unique(&mut libraries, path);
            }
        }
    }
    libraries
}

/// Every place the Documents side might be, as a root and a path relative to it.
fn documents_candidates(
    libraries: &[PathBuf],
    documents_roots: &[PathBuf],
) -> Vec<(PathBuf, String)> {
    let mut candidates = Vec::new();
    for library in libraries {
        // Linux: inside the Proton prefix Steam keeps per-app. Proton has used both names for
        // the user's Documents folder depending on how old the prefix is.
        for documents in ["Documents", "My Documents"] {
            candidates.push((
                library.clone(),
                format!(
                    "steamapps/compatdata/{STEAM_APP_ID}/pfx/drive_c/users/steamuser/{documents}\
                     /My Games/{DOCUMENTS_FOLDER_NAME}"
                ),
            ));
        }
    }
    for root in documents_roots {
        // Windows: straight under the user's Documents folder.
        candidates.push((root.clone(), format!("My Games/{DOCUMENTS_FOLDER_NAME}")));
    }
    candidates
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

/// Resolve `relative` under `root`, returning the path as it really is on disk, or `None` if
/// it is not there.
///
/// Segments are matched exactly first and case-insensitively second. Every tree this walks is
/// a Windows tree - a Steam library holding the Windows game, or a Proton prefix mirroring a
/// Windows drive - where case has never been a difference. Returning the on-disk spelling
/// rather than the one asked for matters: it is the path Sync will later write to.
fn locate(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut current = root.to_path_buf();
    for segment in relative.split('/').filter(|segment| !segment.is_empty()) {
        let exact = current.join(segment);
        if exact.exists() {
            current = exact;
            continue;
        }
        current = case_insensitive_child(&current, segment)?;
    }
    Some(current)
}

fn case_insensitive_child(directory: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(directory).ok()?.flatten() {
        if let Some(found) = entry.file_name().to_str()
            && found.eq_ignore_ascii_case(name)
        {
            return Some(entry.path());
        }
    }
    None
}

/// Which path field a `Browse` click was for.
///
/// Three fields, three different things worth opening at - which is the whole reason this is
/// a Core decision rather than one starting directory for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseField {
    /// The Game Installation field.
    GameInstallation,
    /// The Documents side field.
    Documents,
    /// Dev mode's Community-Patch-DLL checkout field.
    DevCheckout,
}

/// What a `Browse` click has to go on: which field was clicked, what both folder boxes
/// currently hold, and where this machine keeps Steam and the user's home.
///
/// Named fields rather than positional arguments because four of the five are paths and
/// three of those are interchangeable at the type level.
#[derive(Debug, Clone, Copy)]
pub struct BrowseRequest<'a> {
    pub field: BrowseField,
    /// Exactly what the game-folder box holds, trimmed. May be empty, or nonsense.
    pub game_folder: &'a Path,
    /// Exactly what the Documents box holds, trimmed. May be empty, or nonsense.
    pub documents_folder: &'a Path,
    /// Exactly what Dev mode's checkout box holds, trimmed. May be empty, or nonsense.
    pub dev_checkout: &'a Path,
    pub locations: &'a SearchLocations,
    /// The user's home directory, the last rung of the ladder. `None` when the platform
    /// cannot say - the browser then opens wherever it would have anyway.
    pub home: Option<&'a Path>,
}

/// Where a `Browse` click should open the file browser, and what - if anything - it should
/// put in the box on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowseStart {
    /// The directory to open at. `None` only when nothing at all could be worked out, not
    /// even a home directory.
    pub directory: Option<PathBuf>,
    /// A path to write into the box before the browser opens.
    ///
    /// Set only when detection was consulted, which only happens on a path that is not
    /// there - so this can never overwrite a folder the player deliberately chose. It is
    /// written immediately rather than on picking, so cancelling the browser keeps the
    /// correction.
    pub correction: Option<PathBuf>,
}

/// The start-directory ladder: the first rung that answers wins.
///
/// | | Game folder | Documents folder | Dev checkout |
/// |---|---|---|---|
/// | 1 | the box, if that path exists | the box, if that path exists | the box, if that path exists |
/// | 2 | else detection | else detection | - |
/// | 3 | - | else derived from the game-folder box, at its deepest existing part | - |
/// | 4 | else home | else home | else home |
///
/// The checkout has no detection rung: a checkout can be anywhere and there is nothing about
/// this game to detect. It is deliberately not seeded at the Upstream Cache either - that
/// clone is rewritten under the user.
pub fn browse_start(request: BrowseRequest<'_>) -> BrowseStart {
    let box_contents = match request.field {
        BrowseField::GameInstallation => request.game_folder,
        BrowseField::Documents => request.documents_folder,
        BrowseField::DevCheckout => request.dev_checkout,
    };
    // Rung 1. `is_dir` on an empty path is false, so "nothing typed" falls through here.
    if box_contents.is_dir() {
        return BrowseStart {
            directory: Some(box_contents.to_path_buf()),
            correction: None,
        };
    }

    // Rung 2 - only for the two folders detection knows how to look for.
    if let Some(detected) = detected_folder(request.field, request.locations) {
        return BrowseStart {
            directory: Some(detected.clone()),
            correction: Some(detected),
        };
    }

    // Rung 3 - the Documents side derived from the game folder, which on Linux is nine
    // levels inside a Proton prefix and hopeless to reach by hand.
    if request.field == BrowseField::Documents
        && let Some(derived) = documents_near(request.game_folder)
    {
        return BrowseStart {
            directory: Some(derived),
            correction: None,
        };
    }

    // Rung 4.
    BrowseStart {
        directory: request.home.map(Path::to_path_buf),
        correction: None,
    }
}

/// Rung 2: what detection makes of this machine, for the field that was clicked.
fn detected_folder(field: BrowseField, locations: &SearchLocations) -> Option<PathBuf> {
    match (field, detect_game(locations)) {
        // A Game Installation found without its Documents side is still a Game Installation,
        // and it is exactly the case where the player is about to correct the other field.
        (BrowseField::GameInstallation, Detection::Found(game)) => {
            Some(game.game_installation.root().to_path_buf())
        }
        (
            BrowseField::GameInstallation,
            Detection::DocumentsNotFound {
                game_installation, ..
            },
        ) => Some(game_installation.root().to_path_buf()),
        (BrowseField::Documents, Detection::Found(game)) => {
            Some(game.documents.root().to_path_buf())
        }
        // A refused folder is Civilization V but not one this installer can use - offering it
        // as the answer would be putting a known-bad path in the box.
        _ => None,
    }
}

/// Rung 3: the Documents side as it would sit relative to this game folder, opened as deep as
/// the tree actually goes.
///
/// The game folder is not trusted to be `…/steamapps/common/Sid Meier's Civilization V`, so
/// every ancestor of it is tried as a Steam library, deepest first. The candidates are the
/// same ones detection uses, so there is one answer to "where would the Documents side be".
fn documents_near(game_folder: &Path) -> Option<PathBuf> {
    for library in game_folder.ancestors() {
        if !library.is_dir() {
            continue;
        }
        for (root, relative) in documents_candidates(&[library.to_path_buf()], &[]) {
            if let Some(deepest) = deepest_existing(&root, &relative, PREFIX_DEPTH) {
                return Some(deepest);
            }
        }
    }
    None
}

/// How far into a candidate the trail has to go before it is worth following:
/// `steamapps/compatdata/8930`, the Proton prefix Steam makes for this game the first time it
/// is run. Stopping short of that means there is no prefix here at all, only a Steam library -
/// which tells a player looking for their Documents folder nothing that home does not.
const PREFIX_DEPTH: usize = 3;

/// How much of `relative` exists under `root`, as a real path. `None` unless at least `least`
/// of its segments are there.
fn deepest_existing(root: &Path, relative: &str, least: usize) -> Option<PathBuf> {
    let mut deepest = None;
    let mut depth = 0;
    let mut current = root.to_path_buf();
    for segment in relative.split('/').filter(|segment| !segment.is_empty()) {
        let exact = current.join(segment);
        let found = if exact.is_dir() {
            Some(exact)
        } else {
            case_insensitive_child(&current, segment).filter(|found| found.is_dir())
        };
        // The first segment that is not there is where the trail stops: what was walked so
        // far is the deepest real folder on the way to where the answer would be.
        let Some(found) = found else { break };
        current = found;
        depth += 1;
        deepest = Some(current.clone());
    }
    deepest.filter(|_| depth >= least)
}
