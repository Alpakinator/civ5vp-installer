//! How the shell is seen and driven.
//!
//! Two things are checked here, and they are the two halves of "the UI works":
//!
//! * behaviour — the AccessKit tree is queried by label and clicked, exactly as a screen
//!   reader would, and the install that follows is asserted on disk;
//! * looks — every screen is rendered to a committed PNG baseline, so a later change to the
//!   theme shows up as a visual diff rather than as nothing at all (rule 15).
//!
//! Everything is read back out of the accessibility tree. The shell exposes no accessor for
//! its own state (rule 12), so what these tests assert on is what a user can actually see.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use civ5vp_core::{AppDataStore, SearchLocations};
use civ5vp_installer::{InstallerApp, Screen, placeholder};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, SnapshotResults};

/// The size the baselines are rendered at — the window's design minimum. It grew from 640
/// when ticket 10 added the Version picker and the storage panel.
const WINDOW: [f32; 2] = [900.0, 780.0];

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
/// The install runs on a worker thread, so this is a wait, not a single frame. It gives up
/// after about two seconds rather than hanging a test run.
#[track_caller]
fn wait_for_label(harness: &mut Harness<'_, InstallerApp>, text: &str) {
    for _ in 0..200 {
        harness.step();
        if harness.query_all_by_label_contains(text).next().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the shell never showed {text:?}");
}

/// Step until the install has finished, however it ended.
///
/// Waiting for a progress line is not the same thing, and the difference is a real race: the
/// Core reports each Claimed Folder as it lands, so a line naming one arrives while the folders
/// after it — and the Claimed Files after those — are still being written. Anything asserting
/// on disk has to wait for the end, not for a landmark on the way.
///
/// The status line reads "Ready." before an install and "Installing…" during one, and neither
/// afterwards, so "neither of those" is the finished signal. Both are checked because the click
/// is not processed until the next frame.
#[track_caller]
fn wait_for_the_install_to_finish(harness: &mut Harness<'_, InstallerApp>) {
    for _ in 0..400 {
        harness.step();
        let waiting = ["Ready.", "Installing…"]
            .into_iter()
            .any(|status| harness.query_all_by_label(status).next().is_some());
        if !waiting {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the install never finished");
}

/// What a text field currently holds, read out of the accessibility tree the way a screen
/// reader would — the shell exposes no accessor of its own (rule 12).
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
/// Switch the Installation Source to Dev mode and name the checkout — the picker's radio
/// must be clicked first, because the folder field only exists in Dev mode.
fn enter_dev_mode(harness: &mut Harness<'_, InstallerApp>, checkout: &str) {
    harness
        .get_by_label("My own Community-Patch-DLL checkout — Dev mode")
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

/// User story 11: the folders are found, and the player never has to know where they are.
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

    // Progress from the Core reached the shell, including the last line of Sync — the one
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
/// what the install then does — including the EUI Lua strip, which is the change a player is
/// most likely to notice if it silently did not happen.
#[test]
fn picking_vox_populi_with_eui_installs_the_whole_thing() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());

    harness
        .get_by_label("Vox Populi with EUI — adds the Enhanced User Interface")
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

/// The other half of the wiring: turning a toggle off again takes its folder away.
#[test]
fn switching_the_flavor_down_removes_what_no_longer_belongs() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    enter_dev_mode(&mut harness, &miniature_repo().display().to_string());

    harness
        .get_by_label("Vox Populi with EUI — adds the Enhanced User Interface")
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

/// User story 26: what the last run used pre-fills the next one, with nothing to detect.
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
/// that. It used to be dropped — the configuration was remembered whole or not at all, and an
/// unnamed Installation Source made the whole of it unreadable.
#[test]
fn a_flavor_chosen_before_anything_else_is_remembered() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();

    // No source folder typed — only the Flavor and the toggle are touched.
    harness.get_by_label("Community Patch only").click();
    harness
        .get_by_label("43 Civs — room for 43 civilizations on a map")
        .click();
    harness.step();

    let mut next = harness_over(game.launch(&game.nowhere()));
    next.step();
    assert!(
        is_ticked(&mut next, "Community Patch only"),
        "the Flavor should have been remembered",
    );
    assert!(
        is_ticked(&mut next, "43 Civs — room for 43 civilizations on a map"),
        "and so should the toggle",
    );
}

/// User story 14: the native Aspyr port is refused, in words, and nothing can be installed
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

/// User story 12: a wrong folder is caught before anything is written, and the player is told
/// which marker was missing.
#[test]
fn a_wrong_folder_is_rejected_naming_the_marker_that_is_missing() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.nowhere()));
    harness.step();

    // The Documents folder, typed into the game folder field — the mix-up the two similar
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

/// Rule 6, from the outside: a folder that is not a real absolute location is refused, so Sync
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

/// Ticket 08: the Debug choice belongs to Dev mode. Naming a Local Repo is what makes the
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

/// Ticket 10: a new player lands on the GitHub path with the newest Release pre-picked, and
/// the Version they choose instead survives a relaunch.
#[test]
fn the_version_picker_defaults_to_the_newest_release_and_the_pick_is_remembered() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    // The list arrives from a lookup thread — a fixture catalog here, never a socket. The
    // combo exposes its selection as its accessibility *value*, the way a screen reader
    // reads a closed dropdown.
    wait_for_combo_value(&mut harness, "Latest release — Release-5.2");

    // Pick the Latest Development Version through the combo, as a player would.
    harness.get_by_label("Version").click();
    harness.step();
    harness.get_by_label("Latest development version").click();
    harness.step();
    harness.step();

    // A second launch with nowhere to detect: the pick can only have been remembered.
    let mut next = harness_over(game.launch(&game.nowhere()));
    wait_for_combo_value(&mut next, "Latest development version");
}

/// Step until the Version combo's selection reads `text`.
#[track_caller]
fn wait_for_combo_value(harness: &mut Harness<'_, InstallerApp>, text: &str) {
    for _ in 0..200 {
        harness.step();
        if harness
            .query_all_by_label("Version")
            .any(|node| node.value().unwrap_or_default().contains(text))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the Version combo never read {text:?}");
}

/// Ticket 10 / user story 25: the storage panel's clear button empties the App Data Store —
/// and only the store; the game folders are not part of it.
#[test]
fn clear_stored_data_empties_the_app_data_store() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
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

/// User story 24: the Uninstall button returns an unmodded game — Claimed Folders gone,
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

/// Every screen has a baseline. Reviewed before committing — an updated baseline nobody
/// looked at proves nothing (rule 15).
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
