//! Assembling the Modpack.
//!
//! Everything here is plain file work: the Civ5Pkg manifest, the mods copied inside the
//! pack, the UI entry files and their addin hooks, and the emptied overrides of the game's
//! own GameData XML. The one part that needs a database engine - merging the mods' updates
//! into the game's databases and dumping the result - crosses the
//! [`ModpackAssembler`] boundary.
//!
//! The reference for every choice in this file is the Community Patch DLL's own modpack
//! creation (`CvGame::CreateMPMP` and friends) driven by the in-game "Modpack Maker for VP".
//! Where this module and that code disagree, this module is wrong.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::BUILT_DLL_FILE_NAME;
use crate::boundaries::{CacheState, LocalizedText, ModpackAssembler, ModpackDatabaseJob};
use crate::claimed::{ClaimedFolder, GameFolders};
use crate::error::InstallError;
use crate::modinfo;
use crate::plan::Plan;
use crate::progress::{ProgressReporter, Stage};
use crate::tree;

/// The manifest the game requires of every DLC package, byte-for-byte what
/// `CvGame::CreateMPMP` writes. The GUID and Key are the ones every VP modpack in the wild
/// carries; the game checks the pair, so they are not ours to regenerate.
const CIV5PKG_FILE_NAME: &str = "MPModsPack.Civ5Pkg";
const CIV5PKG_CONTENTS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<Civ5Package>\n\
\x20 <GUID>{b5932ae4-0f4f-498f-9333-e2d31b20e095}</GUID>\n\
\x20 <SteamApp>235580</SteamApp>\n\
\x20 <Version>1</Version>\n\
\x20 <Priority>300</Priority>\n\
\x20 <Key>bf6d34a0074b7ad4b1d1716475f7f7fe</Key>\n\
\x20 <PTags>\n\
\x20   <Tag>Version</Tag>\n\
\x20 </PTags>\n\
\x20 <Name>\n\
\x20   <Value language=\"en_US\">VP Modpack</Value>\n\
\x20 </Name>\n\
\x20 <Description>\n\
\x20   <Value language=\"en_US\">Modpack compatible with VP</Value>\n\
\x20 </Description>\n\
\x20 <UISkin name=\"Expansion2Primary\" set=\"Expansion2\" platform=\"Common\">\n\
\x20   <GameplaySkin>\n\
\x20     <Directory>Mods</Directory>\n\
\x20     <Directory>UI</Directory>\n\
\x20   </GameplaySkin>\n\
\x20 </UISkin>\n\
</Civ5Package>\n";

/// The two Override dumps. The names are historical - the game will believe any GameData
/// file name, and the Modpack Maker picked these - but they are what every VP modpack uses,
/// so they stay.
const GAMEPLAY_DUMP_FILE_NAME: &str = "CIV5Units.xml";
const TEXT_DUMP_FILE_NAME: &str = "CIV5Units_Mongol.xml";

/// The UI entry files the game loads from a DLC, and the addin type whose entry points are
/// hooked into each. A mod file with one of these names, at any depth, is copied into `UI/`
/// over the base copy - both facts straight from `CvGame::CopyModFiles`.
const UI_ENTRY_FILES: [(&str, &str); 5] = [
    ("InGame.lua", "InGameUIAddin"),
    ("CityView.lua", "CityViewUIAddin"),
    ("LeaderHeadRoot.lua", "DiplomacyUIAddin"),
    ("MiniMapPanel.lua", "MiniMapOverlayAddin"),
    ("MapGenerator.lua", "PreMapGenScript"),
];

/// The base-game copies the UI entry files start from, relative to the DLC folder.
const BASE_UI_FILES: [(&str, &str); 3] = [
    ("Expansion2/UI/InGame/InGame.lua", "InGame.lua"),
    ("Expansion2/UI/InGame/CityView/CityView.lua", "CityView.lua"),
    (
        "Expansion2/UI/InGame/LeaderHead/LeaderHeadRoot.lua",
        "LeaderHeadRoot.lua",
    ),
];

/// The entry point type of a map script. Not a UI addin: the game lists these in game setup,
/// and a Modpack gets them there through the Documents-side `Maps` folder (ADR-0008).
const MAP_SCRIPT_ADDIN: &str = "MapScript";

/// The in-game Modpack Maker's own mod ID. Excluded from the extra-mod offer the same way
/// that tool excludes itself from its packs: a hotkey listener for building modpacks is
/// dead weight inside one.
const MODPACK_MAKER_ID: &str = "eb8f6ed3-109d-4f2f-a81d-516c8d2f91c1";

/// The player's own mods in the MODS folder, as the extra-mod picker shows them: every
/// folder holding a mod - its `.modinfo` at the top or further down - minus the Claimed
/// Folders, which are the managed set governed by the configuration.
///
/// Only the `offered` ones can be picked. A player must never believe a mod is in their
/// Modpack when part of it is not, so a mod [`bakeable_mod_roots`] refuses is never offered
/// - but it is listed, with the reason, so it does not just vanish from the list.
pub fn extra_mod_offer(mods_folder: &Path) -> ExtraModOffer {
    let mut offer = ExtraModOffer::default();
    let Ok(entries) = std::fs::read_dir(mods_folder) else {
        return offer;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !path.is_dir() || is_claimed_name(&name) {
            continue;
        }
        match bakeable_mod_roots(&path) {
            Ok(_) => offer.offered.push(name),
            Err(Unbakeable::Refused(reason)) => offer.refused.push(RefusedMod { name, reason }),
            Err(Unbakeable::NotOffered(_)) => {}
        }
    }
    offer.offered.sort_unstable();
    offer.refused.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    offer
}

/// What [`extra_mod_offer`] found in the MODS folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraModOffer {
    /// The mods a Modpack can bake in whole, by folder name. Sorted; that is also the order
    /// they are applied in, after the managed set, which is what a modmod patching Vox
    /// Populi expects.
    pub offered: Vec<String>,
    /// The mods it cannot, sorted by folder name.
    pub refused: Vec<RefusedMod>,
}

/// A mod in the MODS folder that a Modpack cannot carry whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedMod {
    /// Its folder name.
    pub name: String,
    /// Why, in the player's words: a clause that reads after "cannot be baked in:".
    pub reason: String,
}

/// Why a folder in MODS cannot be baked into a Modpack.
enum Unbakeable {
    /// Not a mod to show the player at all: a folder with no `.modinfo`, or the Modpack Maker.
    NotOffered(String),
    /// A mod the pack cannot carry whole.
    Refused(String),
}

impl Unbakeable {
    fn reason(&self) -> &str {
        match self {
            Self::NotOffered(reason) | Self::Refused(reason) => reason,
        }
    }
}

/// The mod roots inside one of the player's MODS folders, if every one of them bakes into
/// a Modpack whole; otherwise why not. The offer and the build both go by this, so what
/// the picker shows is exactly what the build can deliver.
fn bakeable_mod_roots(folder: &Path) -> Result<Vec<PathBuf>, Unbakeable> {
    let roots =
        modinfo::mod_roots(folder).map_err(|error| Unbakeable::Refused(error.user_message()))?;
    if roots.is_empty() {
        return Err(Unbakeable::NotOffered("it holds no .modinfo".to_owned()));
    }
    for root in &roots {
        why_not_bakeable(root)?;
    }
    Ok(roots)
}

/// Why this mod root cannot be baked in whole, if it cannot. A Modpack has no Mods menu:
/// only the database updates and the UI addins the pack hooks reach the game, so a mod
/// asking for anything else - or naming a file it does not carry - would arrive partly.
///
/// The reasons are shown to the player beside the mod, so they are in a player's words.
fn why_not_bakeable(root: &Path) -> Result<(), Unbakeable> {
    use Unbakeable::{NotOffered, Refused};
    let modinfo = match modinfo::find(root) {
        Ok(Some(modinfo)) => modinfo,
        Ok(None) => return Err(NotOffered("it holds no .modinfo".to_owned())),
        Err(error) => return Err(Refused(error.user_message())),
    };
    let Ok(text) = std::fs::read_to_string(&modinfo) else {
        return Err(Refused("its .modinfo file cannot be read".to_owned()));
    };
    if modinfo::mod_id(&text).is_some_and(|id| id.eq_ignore_ascii_case(MODPACK_MAKER_ID)) {
        return Err(NotOffered("it is the in-game Modpack Maker".to_owned()));
    }
    if let Some(action) = modinfo::activation_actions(&text)
        .into_iter()
        .find(|action| action != "UpdateDatabase")
    {
        return Err(Refused(format!(
            "it needs the Mods menu to run its {action} step when it is activated"
        )));
    }
    // The game loads one game DLL, and in a Modpack that is the Built DLL.
    let mut own_dll = Vec::new();
    if find_files_named(root, &[BUILT_DLL_FILE_NAME], &mut own_dll).is_ok() && !own_dll.is_empty() {
        return Err(Refused(
            "it brings its own game DLL, and a modpack runs Vox Populi's".to_owned(),
        ));
    }
    let carries = |relative: &str| modinfo::resolve_case_insensitive(root, relative).is_some();
    let missing = |relative: &str| {
        Refused(format!(
            "its .modinfo lists \"{relative}\", which is not in the mod"
        ))
    };
    if let Some(absent) = modinfo::update_database_entries(&text)
        .into_iter()
        .chain(modinfo::listed_files(&text))
        .find(|relative| !carries(relative))
    {
        return Err(missing(&absent));
    }
    for point in modinfo::entry_points(&text) {
        if point.addin_type.eq_ignore_ascii_case(MAP_SCRIPT_ADDIN) {
            if !carries(&point.file) {
                return Err(missing(&point.file));
            }
            continue;
        }
        let Some((ui_file, _)) = UI_ENTRY_FILES
            .iter()
            .find(|(_, addin_type)| addin_type.eq_ignore_ascii_case(&point.addin_type))
        else {
            return Err(Refused(format!(
                "part of it ({}) only runs when the mod is activated in the Mods menu",
                point.file
            )));
        };
        if !carries(&point.file) {
            return Err(missing(&point.file));
        }
        // InGame, CityView and LeaderHeadRoot come from the game; the other entry files
        // exist in a Modpack only if a mod brings them.
        let hook_target_exists = BASE_UI_FILES.iter().any(|(_, base)| base == ui_file) || {
            let mut found = Vec::new();
            find_files_named(root, &[ui_file], &mut found).is_ok() && !found.is_empty()
        };
        if !hook_target_exists {
            return Err(Refused(format!(
                "part of it ({}) plugs into {ui_file}, which nothing in the modpack has",
                point.file
            )));
        }
    }
    Ok(())
}

/// Is this folder name one of the Claimed Folders, under any of its spellings?
fn is_claimed_name(name: &str) -> bool {
    ClaimedFolder::ALL.iter().any(|folder| {
        folder
            .folder_names()
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
    })
}

/// The game cache files a Modpack build starts from, and the snapshot they are kept under
/// in the App Data Store. Snapshotted because the caches do not survive: Sync clears the
/// game's cache folder, and once a Modpack is deployed every later launch rebuilds the
/// caches with the Modpack already applied - only a copy taken while they were pristine
/// lets a second Modpack build (an upgrade) run without asking the player to undo anything.
const GAMEPLAY_BASE_FILE_NAME: &str = "Civ5DebugDatabase.db";
const TEXT_BASE_FILE_NAME: &str = "Localization-Merged.db";

/// What [`assemble`] leaves in the App Data Store for Sync to deploy.
pub(crate) struct AssembledModpack {
    /// The pack itself, deployed as the `VP_MODPACK` Claimed Folder.
    pub(crate) stage: PathBuf,
    /// The map scripts deployed to `Maps/VP_MODPACK` - possibly none.
    pub(crate) map_scripts: PathBuf,
    /// The Text file their scan-time strings go into, if they look any up.
    pub(crate) map_script_text: Option<String>,
}

/// Assemble the complete Modpack in the App Data Store, ready for Sync to deploy.
///
/// `resolved` is [`crate::Core`]'s resolve of the plan's deployments against the
/// materialized source - the same list Sync uses, so what lands inside the Modpack is
/// exactly what a Mods-mode Deployment would have put in MODS.
pub(crate) fn assemble(
    plan: &Plan,
    source_root: &Path,
    resolved: &[(usize, PathBuf)],
    built_dll: &Path,
    work_dir: &Path,
    assembler: &dyn ModpackAssembler,
    progress: &ProgressReporter,
) -> Result<AssembledModpack, InstallError> {
    let (gameplay_base, text_base) =
        ensure_base_snapshot(work_dir, &plan.folders, assembler, progress)?;

    progress.report(Stage::Build, "Assembling the Modpack.");
    let stage = work_dir.join("modpack-stage");
    tree::remove_if_present(&stage)?;
    tree::create_dir_all(&stage)?;
    write_file(&stage.join(CIV5PKG_FILE_NAME), CIV5PKG_CONTENTS)?;

    // The mods, exactly as a Mods-mode Deployment would have deployed them.
    let mods_dir = stage.join("Mods");
    let mut staged_mods = Vec::new();
    for (index, from) in resolved {
        let Some(deployment) = plan.deployments.get(*index) else {
            continue;
        };
        if plan.deploys_directly(deployment) {
            continue;
        }
        let destination = mods_dir.join(deployment.claimed.folder_name());
        tree::copy_selected(from, &destination, &deployment.selection)?;
        staged_mods.push(destination);
    }

    // The player's own picks from MODS, staged after the managed set so their
    // database changes land on top of Vox Populi's, the way a modmod expects. MODS is only
    // read - the copies live inside the pack.
    for name in &plan.configuration.extra_mods {
        if is_claimed_name(name) {
            // A hand-edited configuration could name a managed folder; staging it twice
            // would fight the managed copy, so it is dropped with a word.
            progress.report(
                Stage::Build,
                format!("Skipped \"{name}\" - it is part of the install already."),
            );
            continue;
        }
        let from = plan.folders.mods.join(name);
        if !from.is_dir() {
            return Err(InstallError::UnsupportedConfiguration {
                message: format!(
                    "The mod \"{name}\" is no longer in your MODS folder - untick it, or \
                     put it back and try again. Your game is unchanged."
                ),
                detail: format!("extra mod missing at {}", from.display()),
            });
        }
        // Checked again here, not only when offered: the mod may have changed since it was
        // picked, and a pick the pack cannot carry whole must stop the build, not shrink.
        let roots = bakeable_mod_roots(&from).map_err(|unbakeable| {
            let reason = unbakeable.reason();
            InstallError::UnsupportedConfiguration {
                message: format!(
                    "The mod \"{name}\" cannot be baked into the Modpack: {reason}. Untick \
                     it and try again. Your game is unchanged."
                ),
                detail: format!("extra mod not bakeable at {}: {reason}", from.display()),
            }
        })?;
        let destination = mods_dir.join(name);
        tree::copy_selected(
            &from,
            &destination,
            &crate::plan::SourceSelection::Everything,
        )?;
        progress.report(Stage::Build, format!("Baked {name} into the Modpack."));
        // Each mod root inside the copy - the folder itself, or a nested one - is where
        // its modinfo's paths resolve from.
        for root in roots {
            let inner = root.strip_prefix(&from).unwrap_or(Path::new(""));
            staged_mods.push(destination.join(inner));
        }
    }

    // The Built DLL, where the game's VFS will find it. Every Flavor includes the Community
    // Patch, so the folder is always among the staged mods.
    let dll_home = mods_dir.join(ClaimedFolder::CommunityPatch.folder_name());
    tree::create_dir_all(&dll_home)?;
    tree::copy_file(built_dll, &dll_home.join(BUILT_DLL_FILE_NAME))?;

    stage_ui(&stage, &staged_mods, &plan.folders, progress)?;
    stage_overrides(&stage, &plan.folders)?;
    let map_scripts = work_dir.join("modpack-maps");
    let staged_scripts = stage_map_scripts(&map_scripts, &staged_mods, &plan.folders, progress)?;

    // The in-game Maker sees Documents/Text in its merged localization database. Our
    // saved base predates this Deployment, so it may lack VPUI's tips and EUI option
    // labels (or hold an older Version's). Bake the selected source's Claimed Files into
    // the dump too: distributing the DLC must not depend on the creator's Text folder.
    // Apply these before mod actions, as the game does, so modmods can override them.
    let mut updates: Vec<PathBuf> = plan
        .files
        .iter()
        .filter_map(|file| file.source_path())
        .map(|source_path| source_root.join(source_path))
        .collect();
    for staged in &staged_mods {
        collect_database_updates(staged, &mut updates, progress)?;
    }
    progress.report(
        Stage::Build,
        format!(
            "Merging {} database updates into the game's data.",
            updates.len()
        ),
    );
    let job = ModpackDatabaseJob {
        gameplay_base,
        text_base,
        updates,
        gameplay_dump: stage.join("Override").join(GAMEPLAY_DUMP_FILE_NAME),
        text_dump: stage.join("Override").join(TEXT_DUMP_FILE_NAME),
        scratch_dir: work_dir.join("modpack-scratch"),
        text_tags: text_tags_in(&staged_scripts)?,
    };
    let added_texts = assembler
        .merge_and_dump(&job, progress)
        .map_err(InstallError::Modpack)?;

    progress.report(Stage::Build, "Modpack assembled.");
    Ok(AssembledModpack {
        stage,
        map_scripts,
        map_script_text: (!added_texts.is_empty()).then(|| map_script_text(&added_texts)),
    })
}

/// Stage the map scripts the game would not list for a Modpack: every staged mod's
/// `MapScript` entry point, unless a map of the same file name is already among the game's.
///
/// The pack is not registered DLC and activates no mods, so the game finds map scripts only
/// in its own map directories and the Documents-side `Maps` folder. A pack script named like
/// one of the game's own needs nothing: it overrides that file by name (the "... for VP"
/// entries), and a copy would list it twice. A same-named file in the player's own `Maps`
/// is theirs and stays the one listed. Later mods win a name, as their files do in the pack.
fn stage_map_scripts(
    destination: &Path,
    staged_mods: &[PathBuf],
    folders: &GameFolders,
    progress: &ProgressReporter,
) -> Result<Vec<PathBuf>, InstallError> {
    tree::remove_if_present(destination)?;
    tree::create_dir_all(destination)?;

    let mut scripts: BTreeMap<String, PathBuf> = BTreeMap::new();
    for staged in staged_mods {
        let Some(modinfo) = modinfo::find(staged)? else {
            continue;
        };
        let text = read_file(&modinfo)?;
        for point in modinfo::entry_points(&text) {
            if !point.addin_type.eq_ignore_ascii_case(MAP_SCRIPT_ADDIN) {
                continue;
            }
            // A missing file is the game's business in a Mods Deployment and was refused
            // for the player's own picks; a managed mod's dangling entry lists nothing.
            if let Some(path) = modinfo::resolve_case_insensitive(staged, &point.file)
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
            {
                scripts.insert(name.to_lowercase(), path);
            }
        }
    }

    let game_maps = folders
        .dlc
        .parent()
        .map(|assets| assets.join("Maps"))
        .into_iter()
        .chain(dlc_map_directories(&folders.dlc)?);
    let mut games = BTreeSet::new();
    for directory in game_maps {
        map_file_names(&directory, &mut games)?;
    }
    let mut players = BTreeSet::new();
    map_file_names(&folders.maps(), &mut players)?;

    let mut staged = Vec::new();
    for (key, from) in scripts {
        let Some(name) = from.file_name() else {
            continue;
        };
        if games.contains(&key) {
            continue;
        }
        if players.contains(&key) {
            progress.report(
                Stage::Build,
                format!(
                    "Left out the map script {} - your Maps folder already has one of that \
                     name, and the game lists that one.",
                    name.to_string_lossy()
                ),
            );
            continue;
        }
        let to = destination.join(name);
        tree::copy_file(&from, &to)?;
        staged.push(to);
    }
    if !staged.is_empty() {
        let names: Vec<String> = staged
            .iter()
            .filter_map(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .collect();
        progress.report(
            Stage::Build,
            format!(
                "Listed the map scripts {} for game setup.",
                names.join(", ")
            ),
        );
    }
    Ok(staged)
}

/// Each DLC package's `Maps` folder - where the game's own map packs keep theirs.
fn dlc_map_directories(dlc: &Path) -> Result<Vec<PathBuf>, InstallError> {
    let mut found = Vec::new();
    if !dlc.is_dir() {
        return Ok(found);
    }
    for package in sorted_dir(dlc)? {
        if !package.is_dir() || is_modpack_folder(&package) {
            continue;
        }
        for child in sorted_dir(&package)? {
            if child.is_dir()
                && child
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Maps"))
            {
                found.push(child);
            }
        }
    }
    Ok(found)
}

/// The lowercased names of every map file under `dir`, skipping the Modpack's own folders -
/// a previous Deployment's copies are not the game's.
fn map_file_names(dir: &Path, names: &mut BTreeSet<String>) -> Result<(), InstallError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in sorted_dir(dir)? {
        if entry.is_dir() {
            if !is_modpack_folder(&entry) {
                map_file_names(&entry, names)?;
            }
        } else if let Some(name) = entry.file_name().and_then(|name| name.to_str()) {
            names.insert(name.to_lowercase());
        }
    }
    Ok(())
}

fn is_modpack_folder(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        ClaimedFolder::Modpack
            .folder_names()
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known))
    })
}

/// Every `TXT_KEY_...` literal in these files, sorted and unique. Which of them the game
/// really looks up at scan time does not matter: the assembler returns only strings the
/// merge added, so a tag the game already knows is never written over.
fn text_tags_in(files: &[PathBuf]) -> Result<Vec<String>, InstallError> {
    const PREFIX: &[u8] = b"TXT_KEY_";
    let mut tags = BTreeSet::new();
    for file in files {
        let bytes = std::fs::read(file).map_err(|cause| InstallError::Deployment {
            action: "read",
            path: file.clone(),
            cause,
        })?;
        let mut rest = bytes.as_slice();
        while let Some(start) = rest.windows(PREFIX.len()).position(|w| w == PREFIX) {
            let tail = &rest[start..];
            let length = tail
                .iter()
                .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
                .unwrap_or(tail.len());
            tags.insert(String::from_utf8_lossy(&tail[..length]).into_owned());
            rest = &tail[length..];
        }
    }
    Ok(tags.into_iter().collect())
}

/// The Text Folder file carrying the map scripts' scan-time strings: one `Replace` per
/// string, grouped by language in the order the assembler returned them.
fn map_script_text(texts: &[LocalizedText]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <!-- Written by the Civ 5 VP Installer with the Modpack: the text its map scripts \
         look up while the game lists them. -->\n\
         <GameData>\n",
    );
    let mut open: Option<&str> = None;
    for text in texts {
        if open != Some(text.language.as_str()) {
            if let Some(language) = open {
                xml.push_str(&format!("\t</Language_{language}>\n"));
            }
            xml.push_str(&format!("\t<Language_{}>\n", text.language));
            open = Some(text.language.as_str());
        }
        xml.push_str(&format!(
            "\t\t<Replace Tag=\"{}\">\n\t\t\t<Text>{}</Text>\n\t\t</Replace>\n",
            escape_xml(&text.tag),
            escape_xml(&text.text)
        ));
    }
    if let Some(language) = open {
        xml.push_str(&format!("\t</Language_{language}>\n"));
    }
    xml.push_str("</GameData>\n");
    xml
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The snapshot of the game's pristine databases, taking it now if the game's cache can
/// provide one and it has not been taken before.
fn ensure_base_snapshot(
    work_dir: &Path,
    folders: &GameFolders,
    assembler: &dyn ModpackAssembler,
    progress: &ProgressReporter,
) -> Result<(PathBuf, PathBuf), InstallError> {
    let snapshot = work_dir.join("modpack-base");
    let gameplay = snapshot.join(GAMEPLAY_BASE_FILE_NAME);
    let text = snapshot.join(TEXT_BASE_FILE_NAME);
    if gameplay.is_file() && text.is_file() {
        return Ok((gameplay, text));
    }

    let Some(cache) = folders.cache() else {
        // `GameFolders::check` ran before planning, so this cannot happen - but it must be
        // an error, not a panic, if it somehow does.
        return Err(InstallError::ModpackBaseUnavailable {
            detail: "documents root unresolved after check".to_owned(),
        });
    };
    let cached_gameplay = cache.join(GAMEPLAY_BASE_FILE_NAME);
    let cached_text = cache.join(TEXT_BASE_FILE_NAME);
    if !cached_gameplay.is_file() || !cached_text.is_file() {
        return Err(InstallError::ModpackBaseUnavailable {
            detail: format!("no cache databases at {}", cache.display()),
        });
    }
    match assembler
        .cache_state(&cached_gameplay)
        .map_err(InstallError::Modpack)?
    {
        CacheState::Modded => Err(InstallError::ModpackBaseUnavailable {
            detail: format!(
                "{} was written by a modded session",
                cached_gameplay.display()
            ),
        }),
        CacheState::Pristine => {
            tree::create_dir_all(&snapshot)?;
            tree::copy_file(&cached_gameplay, &gameplay)?;
            tree::copy_file(&cached_text, &text)?;
            progress.report(
                Stage::Build,
                "Saved a copy of the game's own data - future Modpack builds reuse it.",
            );
            Ok((gameplay, text))
        }
    }
}

/// The `UI/` folder: base entry files, mod overrides of them, then the addin hooks.
fn stage_ui(
    stage: &Path,
    staged_mods: &[PathBuf],
    folders: &GameFolders,
    progress: &ProgressReporter,
) -> Result<(), InstallError> {
    let ui = stage.join("UI");
    tree::create_dir_all(&ui)?;

    for (source, name) in BASE_UI_FILES {
        let from = join_relative(&folders.dlc, source);
        if !from.is_file() {
            return Err(InstallError::UnsupportedConfiguration {
                message: format!(
                    "Your game's files are missing {}, which the Modpack is built from. \
                     Verify the game files in Steam, then try again.",
                    from.display()
                ),
                detail: format!("base UI file missing: {}", from.display()),
            });
        }
        tree::copy_file(&from, &ui.join(name))?;
    }

    // A mod file named like a UI entry file replaces it, from any depth - the order is the
    // mods' activation order, matching `CopyModFiles` running per mod. The staged copy is
    // then deleted: the game's VFS keys files by bare name, so a surviving duplicate would
    // race the hooked `UI/` copy for the name, and a pack whose `Mods/` copy wins loses
    // every addin hook. (The in-game Maker leaves the duplicate in; players delete it by
    // hand. The `UI/` copy is a superset, so deleting here can only remove the ambiguity.)
    for staged in staged_mods {
        let mut found = Vec::new();
        find_files_named(staged, &UI_ENTRY_FILES.map(|(name, _)| name), &mut found)?;
        for file in found {
            let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some((canonical_name, _)) = UI_ENTRY_FILES
                .iter()
                .find(|(canonical, _)| canonical.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            // Windows and the game's VFS ignore case; Linux's filesystem does not.
            // Replace the canonical base copy so the hooks go onto this override,
            // rather than leaving both InGame.lua and ingame.lua in the pack.
            tree::copy_file(&file, &ui.join(canonical_name))?;
            tree::remove_file_if_present(&file)?;
        }
    }

    // The hooks: one `g_uiAddins` line per entry point, appended to the UI file its type
    // hooks into. Skipped when the target file does not exist (the Modpack Maker does the
    // same - an addin type nothing provides a base file for is dropped with a log line).
    for staged in staged_mods {
        let Some(modinfo) = modinfo::find(staged)? else {
            continue;
        };
        let text = read_file(&modinfo)?;
        for point in modinfo::entry_points(&text) {
            let Some((_, target)) = UI_ENTRY_FILES
                .iter()
                .find(|(_, addin_type)| addin_type.eq_ignore_ascii_case(&point.addin_type))
                .map(|(file, _)| (file, ui.join(file)))
            else {
                continue;
            };
            let stem = file_stem_of(&point.file);
            if !target.is_file() {
                progress.report(
                    Stage::Build,
                    format!("Skipped the {stem} hook - no UI file to hook it into."),
                );
                continue;
            }
            let line = format!("g_uiAddins[#g_uiAddins + 1] = \"{stem}\";");
            let contents = read_file(&target)?;
            if contents.contains(&line) {
                continue;
            }
            append_file(&target, &format!("\n{line}\n"))?;
        }
    }

    Ok(())
}

/// The `Override/` folder: an empty file for every GameData XML the game would otherwise
/// load. The dumps land in this folder afterwards and carry the whole database instead.
fn stage_overrides(stage: &Path, folders: &GameFolders) -> Result<(), InstallError> {
    let overrides = stage.join("Override");
    tree::create_dir_all(&overrides)?;
    // The DLC folder's parent is the game's `Assets` - the same tree
    // `CvGame::OverrideGamePlayFiles` scans from the game's working directory.
    let Some(assets) = folders.dlc.parent() else {
        return Ok(());
    };
    empty_gamedata_files_under(assets, &overrides)
}

fn empty_gamedata_files_under(dir: &Path, overrides: &Path) -> Result<(), InstallError> {
    for entry in sorted_dir(dir)? {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if entry.is_dir() {
            // The Modpack itself - a previous Deployment's - must not empty its own dumps.
            if name.eq_ignore_ascii_case("VP_MODPACK") {
                continue;
            }
            empty_gamedata_files_under(&entry, overrides)?;
        } else if entry
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
            && holds_gamedata(&entry)
        {
            write_file(&overrides.join(name), "")?;
        }
    }
    Ok(())
}

/// Does the first stretch of this file contain a `<GameData>` tag?
///
/// Mirrors `OverrideGamePlayFiles`: it checks the first 50 lines. Reading a bounded prefix
/// keeps the scan of the game's thousands of XML files cheap.
fn holds_gamedata(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let prefix_end = bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'\n')
        .nth(49)
        .map_or(bytes.len(), |(index, _)| index);
    String::from_utf8_lossy(&bytes[..prefix_end]).contains("<GameData>")
}

/// Every `OnModActivated > UpdateDatabase` file of this staged mod, resolved to real paths,
/// in the order the modinfo lists them.
///
/// An entry naming a file that is not there is skipped with a progress line, not an error:
/// the game does the same (a database.log line, then on with the next action), and upstream
/// really ships such entries - `(1) Community Patch (v 151)` references
/// `CoreGameOptionTextChanges.xml`, renamed to `.sql` with the modinfo action left behind.
/// Refusing would make every affected Version un-Modpackable for everyone.
fn collect_database_updates(
    staged_mod: &Path,
    updates: &mut Vec<PathBuf>,
    progress: &ProgressReporter,
) -> Result<(), InstallError> {
    let Some(modinfo) = modinfo::find(staged_mod)? else {
        return Ok(());
    };
    let text = read_file(&modinfo)?;
    let mod_name = staged_mod
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for relative in modinfo::update_database_entries(&text) {
        let Some(path) = modinfo::resolve_case_insensitive(staged_mod, &relative) else {
            progress.report(
                Stage::Build,
                format!(
                    "Skipped a database update of {mod_name}: its modinfo references \
                     \"{relative}\", which is not among its files - the game skips it the \
                     same way."
                ),
            );
            continue;
        };
        updates.push(path);
    }
    Ok(())
}

/// Every file under `dir` (recursively, sorted walk) whose name matches one of `names`.
fn find_files_named(
    dir: &Path,
    names: &[&str],
    found: &mut Vec<PathBuf>,
) -> Result<(), InstallError> {
    for entry in sorted_dir(dir)? {
        if entry.is_dir() {
            find_files_named(&entry, names, found)?;
        } else if let Some(name) = entry.file_name().and_then(|name| name.to_str())
            && names.iter().any(|n| n.eq_ignore_ascii_case(name))
        {
            found.push(entry);
        }
    }
    Ok(())
}

/// `relative` (with `/` separators) joined onto `root` - segment-wise, so it works on every
/// platform, and case-insensitively, because the game's own files are cased however the
/// installation shipped them.
fn join_relative(root: &Path, relative: &str) -> PathBuf {
    modinfo::resolve_case_insensitive(root, relative).unwrap_or_else(|| {
        relative
            .split('/')
            .fold(root.to_path_buf(), |p, s| p.join(s))
    })
}

/// The file name without its extension - `Path::file_stem`, tolerant of Windows separators
/// in modinfo values.
fn file_stem_of(file: &str) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    match name.rsplit_once('.') {
        Some((stem, _)) => stem.to_owned(),
        None => name.to_owned(),
    }
}

fn sorted_dir(dir: &Path) -> Result<Vec<PathBuf>, InstallError> {
    let entries = std::fs::read_dir(dir).map_err(|cause| InstallError::Deployment {
        action: "read",
        path: dir.to_path_buf(),
        cause,
    })?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort_unstable();
    Ok(paths)
}

fn read_file(path: &Path) -> Result<String, InstallError> {
    std::fs::read_to_string(path).map_err(|cause| InstallError::Deployment {
        action: "read",
        path: path.to_path_buf(),
        cause,
    })
}

fn write_file(path: &Path, contents: &str) -> Result<(), InstallError> {
    std::fs::write(path, contents).map_err(|cause| InstallError::Deployment {
        action: "write",
        path: path.to_path_buf(),
        cause,
    })
}

fn append_file(path: &Path, contents: &str) -> Result<(), InstallError> {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .map_err(|cause| InstallError::Deployment {
            action: "append",
            path: path.to_path_buf(),
            cause,
        })
}
