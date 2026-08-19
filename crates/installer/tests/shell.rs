//! How the shell is seen and driven.
//!
//! Two things are checked here, and they are the two halves of "the UI works":
//!
//! * behaviour - the AccessKit tree is queried by label and clicked, exactly as a screen
//!   reader would, and the install that follows is asserted on disk;
//! * looks - every screen is rendered to a committed PNG baseline, so a later change to the
//!   theme shows up as a visual diff rather than as nothing at all.
//!
//! Everything is read back out of the accessibility tree. The shell exposes no accessor for
//! its own state, so what these tests assert on is what a user can actually see.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use civ5vp_core::{AppDataStore, LuaJitEngine, SearchLocations};
use civ5vp_installer::{InstallerApp, Screen, placeholder};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, SnapshotResults};

/// The size the baselines are rendered at - the window's design minimum.
const WINDOW: [f32; 2] = [900.0, 990.0];

/// The same miniature Community-Patch-DLL layout the Core-seam tests use. Shared rather
/// than duplicated so there is one answer to "what does a repository look like".
fn miniature_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/miniature-repo")
}

fn harness_over(app: InstallerApp) -> Harness<'static, InstallerApp> {
    Harness::builder()
        .with_size(egui::Vec2::from(WINDOW))
        .build_ui_state(|ui, app: &mut InstallerApp| app.show(ui), app)
}

/// A Steam library holding a real-shaped Civilization V install and its Proton prefix, plus an
/// App Data Store, all in a temporary directory.
///
/// The layout has to be the real one, marker for marker, because the shell now asks the Core
/// whether it is real before it will install anything.
struct TempGame {
    temp: tempfile::TempDir,
}

impl TempGame {
    fn new() -> Self {
        let fixture = Self {
            temp: tempfile::tempdir().unwrap(),
        };

        let game = fixture.game_folder();
        fs::create_dir_all(game.join("Assets/DLC/Expansion2")).unwrap();
        fs::write(game.join("CivilizationV.exe"), "not really an executable").unwrap();
        fs::write(
            game.join("CivilizationV_DX11.exe"),
            "not really an executable",
        )
        .unwrap();

        let documents = fixture.documents_folder();
        for folder in ["MODS", "Text", "ModUserData"] {
            fs::create_dir_all(documents.join(folder)).unwrap();
        }
        fs::write(documents.join("UserSettings.ini"), "[Game]\n").unwrap();

        fixture
    }

    fn library(&self) -> PathBuf {
        self.temp.path().join("Steam")
    }

    fn game_folder(&self) -> PathBuf {
        self.library()
            .join("steamapps/common/Sid Meier's Civilization V")
    }

    fn documents_folder(&self) -> PathBuf {
        self.library().join(
            "steamapps/compatdata/8930/pfx/drive_c/users/steamuser/Documents/My Games/\
             Sid Meier's Civilization 5",
        )
    }

    fn mods_folder(&self) -> PathBuf {
        self.documents_folder().join("MODS")
    }

    fn store(&self) -> AppDataStore {
        AppDataStore::at(self.temp.path().join("app-data"))
    }

    fn core(&self) -> Arc<civ5vp_core::Core> {
        Arc::new(placeholder::core(self.temp.path().join("app-data")))
    }

    /// Where detection is allowed to look: this fixture and nothing else, so no test ever
    /// consults the Steam install of whoever is running it.
    fn locations(&self) -> SearchLocations {
        SearchLocations {
            steam_roots: vec![self.library()],
            documents_roots: Vec::new(),
        }
    }

    /// A launch that can find nothing at all.
    fn nowhere(&self) -> SearchLocations {
        SearchLocations::default()
    }

    fn launch(&self, locations: &SearchLocations) -> InstallerApp {
        InstallerApp::launch(self.core(), self.store(), locations)
    }

    fn temp_path(&self) -> &Path {
        self.temp.path()
    }
}

/// Step the UI until `text` shows up somewhere in the accessibility tree.
///
/// The install runs on a worker thread, so this is a wait, not a single frame. The budget
/// is generous because CI runners are slow and heavily shared; on timeout the panic says
/// what *was* on screen, so a remote failure is diagnosable from its log alone.
#[track_caller]
fn wait_for_label(harness: &mut Harness<'_, InstallerApp>, text: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        harness.step();
        if harness.query_all_by_label_contains(text).next().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "the shell never showed {text:?}; visible: {:?}",
        visible_labels(harness)
    );
}

/// Every label currently in the accessibility tree - the timeout panics print this, so a
/// flaky CI failure carries its own screenshot-in-words.
fn visible_labels(harness: &mut Harness<'_, InstallerApp>) -> Vec<String> {
    use egui_kittest::kittest::NodeT as _;
    harness
        .query_all_by_label_contains("")
        .filter_map(|node| node.accesskit_node().label())
        .collect()
}

/// Step until the install has finished, however it ended.
///
/// Waiting for a progress line is not the same thing, and the difference is a real race: the
/// Core reports each Claimed Folder as it lands, so a line naming one arrives while the folders
/// after it - and the Claimed Files after those - are still being written. Anything asserting
/// on disk has to wait for the end, not for a landmark on the way.
///
/// The status line reads "Ready." before an install and "Installing…" during one, and neither
/// afterwards, so "neither of those" is the finished signal. Both are checked because the click
/// is not processed until the next frame.
#[track_caller]
fn wait_for_the_install_to_finish(harness: &mut Harness<'_, InstallerApp>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        harness.step();
        let waiting = ["Ready.", "Installing…"]
            .into_iter()
            .any(|status| harness.query_all_by_label(status).next().is_some());
        if !waiting {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "the install never finished; visible: {:?}",
        visible_labels(harness)
    );
}

/// What a text field currently holds, read out of the accessibility tree the way a screen
/// reader would - the shell exposes no accessor of its own.
#[track_caller]
fn field_value(harness: &mut Harness<'_, InstallerApp>, label: &str) -> String {
    harness.get_by_label(label).value().unwrap_or_default()
}

/// Whether a radio button or checkbox is on, read the way a screen reader reads it.
#[track_caller]
fn is_ticked(harness: &mut Harness<'_, InstallerApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT as _;
    harness.get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[track_caller]
/// Switch the Installation Source to Dev mode and name the checkout - the picker's radio
/// must be clicked first, because the folder field only exists in Dev mode.
fn enter_dev_mode(harness: &mut Harness<'_, InstallerApp>, checkout: &str) {
    harness
        .get_by_label("My own Community-Patch-DLL checkout - Dev mode")
        .click();
    harness.step();
    set_text(harness, "Community-Patch-DLL folder", checkout);
}

fn set_text(harness: &mut Harness<'_, InstallerApp>, label: &str, value: &str) {
    let field = harness.get_by_label(label);
    field.focus();
    field.type_text(value);
    harness.step();
}

/// The folders are found, and the player never has to know where they are.
#[test]
fn a_launch_pre_fills_the_folders_it_detects() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    // The two folders the player would otherwise have had to find…
    assert_eq!(
        field_value(&mut harness, "Civilization V game folder"),
        game.game_folder().display().to_string(),
    );
    assert_eq!(
        field_value(&mut harness, "Civilization 5 Documents folder"),
        game.documents_folder().display().to_string(),
    );
    // …and the three the Core derives from them.
    for expected in [
        format!("MODS folder: {}", game.mods_folder().display()),
        format!(
            "DLC folder: {}",
            game.game_folder().join("Assets/DLC").display()
        ),
        format!(
            "Text folder: {}",
            game.documents_folder().join("Text").display()
        ),
    ] {
        assert!(
            harness.query_by_label(&expected).is_some(),
            "expected the shell to show {expected:?}",
        );
    }
}

/// An install driven the way a user drives it, into the folders detection found, with the
/// smallest Flavor picked explicitly.
///
/// The Flavor is chosen rather than left at its default on purpose: the default is Vox Populi
/// with EUI, so a test that did not click would be asserting about a much larger install than
/// its name suggests.
#[test]
fn clicking_install_deploys_the_community_patch() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));

    harness.step();
    assert!(
        harness.query_by_label("Ready.").is_some(),
        "the shell should start out idle",
    );

    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());
    harness.get_by_label("Community Patch only").click();
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);
    assert!(
        harness
            .query_all_by_label("Installed (1) Community Patch.")
            .next()
            .is_some(),
        "the shell should report what it installed",
    );

    assert!(
        !game.mods_folder().join("(2) Vox Populi").exists(),
        "Community Patch only should not have deployed Vox Populi",
    );

    // Progress from the Core reached the shell, including the last line of Sync - the one
    // that arrives in the same breath as the result.
    assert!(
        harness
            .query_all_by_label_contains("Installing into the game: Installed the DLL.")
            .next()
            .is_some(),
        "the shell should have shown the Core's progress, down to the last event",
    );

    let deployed = game.mods_folder().join("(1) Community Patch");
    assert!(deployed.join("(1) Community Patch.modinfo").is_file());
    assert_eq!(
        fs::read_to_string(deployed.join("CvGameCore_Expansion2.dll")).unwrap(),
        placeholder::PLACEHOLDER_DLL_CONTENTS,
    );
}

/// Picking a Flavor in the UI reaches the Core and changes what lands on disk.
///
/// The matrix is the Core's, and `matrix.rs` asserts it exhaustively. What this proves is the
/// wiring: that the three radio buttons are the three legal Flavors and that choosing one is
/// what the install then does - including the EUI Lua strip, which is the change a player is
/// most likely to notice if it silently did not happen.
#[test]
fn picking_vox_populi_with_eui_installs_the_whole_thing() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());

    harness
        .get_by_label("Community Patch + Vox Populi + EUI")
        .click();
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);

    let mods = game.mods_folder();
    let dlc = game.game_folder().join("Assets/DLC");
    for expected in [
        mods.join("(1) Community Patch/(1) Community Patch.modinfo"),
        mods.join("(2) Vox Populi/(2) Vox Populi.modinfo"),
        mods.join("(3a) VP - EUI Compatibility Files/LUA/EUI_core_library.lua"),
        mods.join("(4a) Squads for VP/UI/Squads.lua"),
        dlc.join("VPUI/VPUI_0.Civ5Pkg"),
        dlc.join("UI_bc1/EUI_0.Civ5Pkg"),
        game.documents_folder().join("Text/VPUI_tips_en_us.xml"),
    ] {
        assert!(
            expected.is_file(),
            "expected {} to be installed",
            expected.display()
        );
    }
    assert!(
        !mods.join("(1) Community Patch/LUA").exists(),
        "EUI replaces the Lua in (1), so the original must not be there",
    );
}

/// The Modpack mode radio drives a Modpack Deployment - the pack lands in the
/// game's DLC, MODS is left alone, and the choice survives a relaunch.
#[test]
fn picking_the_modpack_mode_builds_a_modpack_and_is_remembered() {
    let game = TempGame::new();
    // What a Modpack build needs of the game: the base UI entry files and a pristine cache
    // (the placeholder assembler reads the marker, exactly like the Core-seam fixture).
    let expansion_ui = game.game_folder().join("Assets/DLC/Expansion2/UI/InGame");
    fs::create_dir_all(expansion_ui.join("CityView")).unwrap();
    fs::create_dir_all(expansion_ui.join("LeaderHead")).unwrap();
    fs::write(expansion_ui.join("InGame.lua"), "-- base InGame\n").unwrap();
    fs::write(
        expansion_ui.join("CityView/CityView.lua"),
        "-- base CityView\n",
    )
    .unwrap();
    fs::write(
        expansion_ui.join("LeaderHead/LeaderHeadRoot.lua"),
        "-- base LeaderHeadRoot\n",
    )
    .unwrap();
    let cache = game.documents_folder().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("Civ5DebugDatabase.db"), "pristine base").unwrap();
    fs::write(cache.join("Localization-Merged.db"), "pristine text").unwrap();
    // A mod install from an earlier day, which the Modpack Deployment must leave alone.
    fs::create_dir_all(game.mods_folder().join("(2) Vox Populi")).unwrap();
    fs::write(
        game.mods_folder().join("(2) Vox Populi/existing.txt"),
        "left alone",
    )
    .unwrap();
    // The player's own mod, offered as an extra pick.
    fs::create_dir_all(game.mods_folder().join("My Modmod")).unwrap();
    fs::write(
        game.mods_folder().join("My Modmod/My Modmod.modinfo"),
        "<Mod id=\"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\" version=\"1\"/>",
    )
    .unwrap();
    fs::write(game.mods_folder().join("My Modmod/tweak.lua"), "-- mine").unwrap();

    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());
    harness.get_by_label("Community Patch + Vox Populi").click();
    harness
        .get_by_label("Install as a modpack - loads automatically, works in multiplayer")
        .click();
    harness.step();
    harness.get_by_label("My Modmod").click();
    // Modpack mode draws the longest page there is - its explanation, plus a row for every
    // mod the player could bake in - so at the design-minimum window size Install sits below
    // the fold. The page scrolls by design, so this presses the button the way a screen
    // reader does, through the accessibility tree, rather than by aiming at a pixel.
    harness.get_by_label("Install").click_accesskit();
    wait_for_the_install_to_finish(&mut harness);

    let pack = game.game_folder().join("Assets/DLC/VP_MODPACK");
    assert!(
        pack.join("MPModsPack.Civ5Pkg").is_file(),
        "the modpack should be deployed into the game's DLC"
    );
    assert_eq!(
        fs::read_to_string(pack.join("Mods/(1) Community Patch/CvGameCore_Expansion2.dll"))
            .unwrap(),
        placeholder::PLACEHOLDER_DLL_CONTENTS,
    );
    assert_eq!(
        fs::read_to_string(pack.join("Override/CIV5Units.xml")).unwrap(),
        placeholder::PLACEHOLDER_DUMP_CONTENTS,
    );
    assert_eq!(
        fs::read_to_string(game.mods_folder().join("(2) Vox Populi/existing.txt")).unwrap(),
        "left alone",
        "a Modpack Deployment leaves MODS untouched"
    );
    assert!(
        !game.mods_folder().join("(1) Community Patch").exists(),
        "the mods go inside the pack, not into MODS"
    );

    // The player's pick was baked in beside the managed set, and the original only read.
    assert_eq!(
        fs::read_to_string(pack.join("Mods/My Modmod/tweak.lua")).unwrap(),
        "-- mine",
    );
    assert!(game.mods_folder().join("My Modmod/tweak.lua").is_file());

    // The mode and the pick are remembered like every other part of the configuration -
    // but the pick needs the mod to still be there, so this relaunch
    // detects the same folders.
    let mut next = harness_over(game.launch(&game.locations()));
    next.step();
    assert!(is_ticked(
        &mut next,
        "Install as a modpack - loads automatically, works in multiplayer"
    ));
    assert!(is_ticked(&mut next, "My Modmod"));
}

/// The other half of the wiring: turning a toggle off again takes its folder away.
#[test]
fn switching_the_flavor_down_removes_what_no_longer_belongs() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());

    harness
        .get_by_label("Community Patch + Vox Populi + EUI")
        .click();
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);
    assert!(game.mods_folder().join("(2) Vox Populi").exists());

    harness.get_by_label("Community Patch only").click();
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);

    for gone in [
        game.mods_folder().join("(2) Vox Populi"),
        game.mods_folder().join("(3a) VP - EUI Compatibility Files"),
        game.game_folder().join("Assets/DLC/UI_bc1"),
        game.documents_folder().join("Text/VPUI_tips_en_us.xml"),
    ] {
        assert!(
            !gone.exists(),
            "{} should have been removed",
            gone.display()
        );
    }
    assert!(
        game.mods_folder()
            .join("(1) Community Patch/LUA/CityView.lua")
            .is_file(),
        "and the Lua EUI had replaced should be back",
    );
}

/// What the last run used pre-fills the next one, with nothing to detect.
#[test]
fn what_one_launch_settles_the_next_launch_starts_from() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);

    // A second launch of the same installer, with nowhere to search: everything it shows can
    // only have come out of the App Data Store.
    let mut next = harness_over(game.launch(&game.nowhere()));
    next.step();

    assert!(
        next.query_by_label(&format!("MODS folder: {}", game.mods_folder().display()))
            .is_some(),
        "the remembered folders should have pre-filled the next launch",
    );
    assert_eq!(
        field_value(&mut next, "Community-Patch-DLL folder"),
        miniature_repo().display().to_string(),
        "the remembered Install Configuration should have pre-filled the next launch",
    );
}

/// A Flavor picked before anything has been installed is still there next launch.
///
/// This is the shell half of the same rule the Core tests cover: on a first run nobody has
/// pointed the installer at any sources yet, and the Flavor they did choose has to survive
/// that. It used to be dropped - the configuration was remembered whole or not at all, and an
/// unnamed Installation Source made the whole of it unreadable.
#[test]
fn a_flavor_chosen_before_anything_else_is_remembered() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    // No source folder typed - only the Flavor and the toggle are touched.
    harness.get_by_label("Community Patch only").click();
    harness
        .get_by_label("43 Civs - room for 43 civilizations on a map")
        .click();
    harness.step();

    let mut next = harness_over(game.launch(&game.nowhere()));
    next.step();
    assert!(
        is_ticked(&mut next, "Community Patch only"),
        "the Flavor should have been remembered",
    );
    assert!(
        is_ticked(&mut next, "43 Civs - room for 43 civilizations on a map"),
        "and so should the toggle",
    );
}

/// The engine choice reaches the Core, and the default leaves the game's engine alone.
///
/// The configuration is read back out of the App Data Store rather than out of the app,
/// because that is the same `InstallConfiguration` the shell hands to a Deployment - the
/// remembered copy is built by the very call an Install makes.
#[test]
fn the_luajit_checkbox_reaches_the_configuration() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    assert_eq!(
        remembered_configuration(&game).luajit,
        LuaJitEngine::Stock,
        "replacing a game file must never be the default",
    );

    harness.get_by_label("Use the LuaJIT engine").click();
    harness.step();
    assert_eq!(
        remembered_configuration(&game).luajit,
        LuaJitEngine::LuaJit,
        "a ticked checkbox should ask the Core for the LuaJIT engine",
    );

    // And like every other part of the configuration, the choice survives the session.
    let mut next = harness_over(game.launch(&game.nowhere()));
    next.step();
    assert!(is_ticked(&mut next, "Use the LuaJIT engine"));
}

/// Nobody is opted into overwriting a file the game owns.
///
/// ADR-0006's third Replaced-File rule, drawn: a launch that has never seen a settings file
/// offers the engine option unticked, and the configuration it remembers says so.
#[test]
fn the_game_engine_is_left_alone_unless_a_player_asks_for_it() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    assert!(
        !is_ticked(&mut harness, "Use the LuaJIT engine"),
        "the engine checkbox should open unticked",
    );
    assert_eq!(remembered_configuration(&game).luajit, LuaJitEngine::Stock);
}

/// The Install Configuration a launch left in the App Data Store.
fn remembered_configuration(game: &TempGame) -> civ5vp_core::InstallConfiguration {
    let Ok(settings) = game.store().load() else {
        unreachable!("a launch over resolved folders remembers them")
    };
    let Some(configuration) = settings.configuration else {
        unreachable!("and what it remembers includes the Install Configuration")
    };
    configuration
}

/// The native Aspyr port is refused, in words, and nothing can be installed
/// into it.
#[test]
fn the_native_linux_port_is_refused_with_an_explanation() {
    let game = TempGame::new();
    // Turn the fixture into the native port: the Aspyr binaries, and none of the Windows ones.
    let root = game.game_folder();
    fs::remove_file(root.join("CivilizationV.exe")).unwrap();
    fs::remove_file(root.join("CivilizationV_DX11.exe")).unwrap();
    fs::write(root.join("Civ5XP"), "the Aspyr port's binary").unwrap();

    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    assert!(
        harness
            .query_all_by_label_contains("native Linux version of Civilization V from Aspyr")
            .next()
            .is_some(),
        "the shell should explain why this game cannot be used",
    );

    harness.get_by_label("Install").click();
    harness.step();
    assert!(
        !game.mods_folder().join("(1) Community Patch").exists(),
        "nothing should have been written into a refused game",
    );
}

/// A wrong folder is caught before anything is written, and the player is told
/// which marker was missing.
#[test]
fn a_wrong_folder_is_rejected_naming_the_marker_that_is_missing() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.nowhere()));
    harness.step();

    // The Documents folder, typed into the game folder field - the mix-up the two similar
    // names invite.
    set_text(
        &mut harness,
        "Civilization V game folder",
        &game.documents_folder().display().to_string(),
    );
    set_text(
        &mut harness,
        "Civilization 5 Documents folder",
        &game.documents_folder().display().to_string(),
    );

    assert!(
        harness
            .query_all_by_label_contains("it has no CivilizationV.exe in it")
            .next()
            .is_some(),
        "the shell should name the marker that was missing",
    );

    harness.get_by_label("Install").click();
    harness.step();
    assert!(
        !game.mods_folder().join("(1) Community Patch").exists(),
        "nothing should have been written",
    );
}

/// A folder that is not a real absolute location is refused, so Sync
/// never aims its deletes at whatever the working directory happens to be.
#[test]
fn a_relative_folder_is_refused_before_anything_is_written() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.nowhere()));
    harness.step();

    set_text(
        &mut harness,
        "Civilization V game folder",
        "Sid Meier's Civilization V",
    );

    assert!(
        harness
            .query_all_by_label_contains("needs to be a full path")
            .next()
            .is_some(),
        "a relative folder should be refused in words",
    );

    harness.get_by_label("Install").click();
    harness.step();
    assert!(
        !Path::new("Sid Meier's Civilization V").exists(),
        "a relative game folder must never be created next to the process",
    );
}

/// A failure the user can act on, not a stack trace: the source folder does not exist.
#[test]
fn a_bad_source_folder_is_explained_and_nothing_is_installed() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, "/no/such/checkout");

    harness.get_by_label("Install").click();
    wait_for_label(&mut harness, "There is no folder at");

    assert!(
        !game.mods_folder().join("(1) Community Patch").exists(),
        "nothing should have been written to the game",
    );
}

/// A run that failed must not sign off with the word players read as success.
///
/// This is the bug behind a real report: a download died part-way, the Activity panel's last
/// line still read "Finished in 4 min 50 s", and the player believed the mod was installed.
#[test]
fn a_failed_run_says_it_stopped_rather_than_that_it_finished() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, "/no/such/checkout");

    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);
    wait_for_label(&mut harness, "Stopped after");

    assert!(
        harness
            .query_all_by_label_contains("Finished in")
            .next()
            .is_none(),
        "a failed run must not claim to have finished; visible: {:?}",
        visible_labels(&mut harness)
    );
}

/// The Debug choice belongs to Dev mode. Naming a Local Repo is what makes the
/// checkbox exist; without one it is not drawn at all.
#[test]
fn the_debug_choice_appears_only_in_dev_mode() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    assert!(
        harness
            .query_all_by_label_contains("Debug build")
            .next()
            .is_none(),
        "no Local Repo named yet, so no Debug choice"
    );

    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());
    harness.step();

    assert!(
        harness
            .query_all_by_label_contains("Debug build")
            .next()
            .is_some(),
        "a named checkout is Dev mode, and Dev mode has the Debug choice"
    );
}

/// A new player lands on the GitHub path with the newest Release pre-picked, and
/// the Version they choose instead survives a relaunch.
#[test]
fn the_version_picker_defaults_to_the_newest_release_and_the_pick_is_remembered() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    // The list arrives from a lookup thread - a fixture catalog here, never a socket. The
    // combo exposes its selection as its accessibility *value*, the way a screen reader
    // reads a closed dropdown.
    wait_for_combo_value(&mut harness, "Latest release - Release-5.2");

    // Pick an older Release through the combo, as a player would.
    harness.get_by_label("Version").click();
    harness.step();
    // The open list names the newest release once - inside "Latest release - …" - and
    // never again as a bare entry; older releases keep their own rows.
    assert_eq!(
        harness.query_all_by_label_contains("Release-5.2").count(),
        1,
        "the newest release must not be listed twice"
    );
    harness.get_by_label("Release-5.1").click();
    harness.step();
    harness.step();

    // A second launch with nowhere to detect: the pick can only have been remembered.
    let mut next = harness_over(game.launch(&game.nowhere()));
    wait_for_combo_value(&mut next, "Release-5.1");
}

/// The picker offers official Releases only, until the unofficial toggle brings
/// in the changes since the newest one - truncated to fit, newest first - and a picked
/// build is remembered like any other Version.
#[test]
fn the_unofficial_toggle_lists_the_changes_since_the_newest_release() {
    use egui_kittest::kittest::NodeT as _;
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    wait_for_combo_value(&mut harness, "Latest release - Release-5.2");

    // Off by default - and "latest development version" is no longer an offer either.
    harness.get_by_label("Version").click();
    harness.step();
    assert!(
        harness
            .query_all_by_label("Latest development version")
            .next()
            .is_none(),
        "development is not offered by default"
    );
    assert!(
        harness
            .query_all_by_label_contains("5.2.01")
            .next()
            .is_none(),
        "unofficial versions are not offered by default"
    );
    harness.get_by_label("Version").click();
    harness.step();

    harness
        .get_by_label("Unofficial versions - every change since the newest release")
        .click();
    harness.step();
    harness.get_by_label("Version").click();
    for _ in 0..100 {
        harness.step();
        if harness
            .query_all_by_label_contains("5.2.01")
            .next()
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // The long commit message is truncated in the row, with an ellipsis.
    let row_label = harness
        .query_all_by_label_contains("5.2.02")
        .next()
        .expect("the second unofficial build is listed")
        .accesskit_node()
        .label()
        .unwrap_or_default();
    assert!(row_label.contains('…'), "got: {row_label}");
    assert!(
        row_label.chars().count() < 60,
        "the row must be truncated, got {} chars",
        row_label.chars().count()
    );

    harness.get_by_label_contains("5.2.01").click();
    harness.step();
    harness.step();
    wait_for_combo_value(&mut harness, "5.2.01");

    // Remembered across a relaunch - and the toggle comes back on with it.
    let mut next = harness_over(game.launch(&game.nowhere()));
    wait_for_combo_value(&mut next, "5.2.01");
    assert!(is_ticked(
        &mut next,
        "Unofficial versions - every change since the newest release"
    ));
}

/// Step until the Version combo's selection reads `text`.
#[track_caller]
fn wait_for_combo_value(harness: &mut Harness<'_, InstallerApp>, text: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        harness.step();
        if harness
            .query_all_by_label("Version")
            .any(|node| node.value().unwrap_or_default().contains(text))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "the Version combo never read {text:?}; visible: {:?}",
        visible_labels(harness)
    );
}

/// The storage panel's clear button empties the App Data Store -
/// and only the store; the game folders are not part of it.
#[test]
fn clear_stored_data_empties_the_app_data_store() {
    let game = TempGame::new();
    // The Storage panel sits at the very bottom; a player scrolls to it, but a test can
    // only click what is on screen - so this harness is simply tall enough.
    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(WINDOW[0], 1200.0))
        .build_ui_state(
            |ui, app: &mut InstallerApp| app.show(ui),
            game.launch(&game.locations()),
        );
    harness.step();
    let store_root = game.temp_path().join("app-data");
    assert!(
        std::fs::read_dir(&store_root).unwrap().count() > 0,
        "the launch remembered settings, so the store is not empty"
    );

    harness.get_by_label("Storage").click();
    harness.step();
    harness.get_by_label("Clear stored data").click();
    harness.step();

    assert_eq!(
        std::fs::read_dir(&store_root).unwrap().count(),
        0,
        "the store is emptied"
    );
    assert!(
        game.game_folder().join("CivilizationV.exe").exists(),
        "the game is untouched"
    );
}

/// The Uninstall button returns an unmodded game - Claimed Folders gone,
/// everything else (the game itself, ModUserData) untouched.
#[test]
fn clicking_uninstall_restores_an_unmodded_game() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);
    assert!(game.mods_folder().join("(1) Community Patch").is_dir());

    harness.get_by_label("Uninstall").click();
    wait_for_label(&mut harness, "back to how it was");

    assert!(
        !game.mods_folder().join("(1) Community Patch").exists(),
        "the Claimed Folders are removed"
    );
    assert!(
        game.game_folder().join("CivilizationV.exe").exists(),
        "the game itself is untouched"
    );
    assert!(
        game.documents_folder().join("ModUserData").exists(),
        "ModUserData survives an Uninstall"
    );
}

/// Every screen has a baseline. Reviewed before committing - an updated baseline nobody
/// looked at proves nothing.
#[test]
fn every_screen_matches_its_baseline() {
    let mut results = SnapshotResults::new();
    for screen in Screen::ALL {
        let mut harness = harness_over(InstallerApp::preview(screen));
        harness.run();
        results.add(harness.try_snapshot(screen.file_stem()));
    }
    results.unwrap();
}

/// A checkout named once is pre-filled forever - even after the player switches back to the
/// GitHub source and relaunches, Dev mode opens with the remembered path in the field.
#[test]
fn the_dev_checkout_survives_a_switch_back_to_github_and_a_relaunch() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());

    // Back to GitHub - the configuration now stores the GitHub source, not the checkout.
    harness
        .get_by_label("Download from GitHub - pick a version")
        .click();
    harness.step();
    harness.step();

    // A fresh launch: Dev mode must open with the path still there.
    let mut next = harness_over(game.launch(&game.nowhere()));
    next.step();
    next.get_by_label("My own Community-Patch-DLL checkout - Dev mode")
        .click();
    next.step();
    assert_eq!(
        field_value(&mut next, "Community-Patch-DLL folder"),
        miniature_repo().display().to_string(),
    );
}

/// The first-run cost is on screen before the click, while the toolchain
/// would still have to be downloaded, and the sentence disappears the moment an install
/// has made it untrue.
#[test]
fn the_first_run_cost_is_announced_until_the_toolchain_exists() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    assert!(
        harness
            .query_all_by_label_contains("First install downloads about")
            .next()
            .is_some(),
        "a fresh App Data Store must announce the first-run cost"
    );

    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());
    harness.get_by_label("Community Patch only").click();
    harness.get_by_label("Install").click();
    wait_for_the_install_to_finish(&mut harness);
    assert!(
        harness
            .query_all_by_label_contains("First install downloads about")
            .next()
            .is_none(),
        "once the toolchain exists the warning must stop"
    );

    // And it stays gone on the next launch.
    let mut next = harness_over(game.launch(&game.nowhere()));
    next.step();
    assert!(
        next.query_all_by_label_contains("First install downloads about")
            .next()
            .is_none()
    );
}
