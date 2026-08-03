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

/// The size the baselines are rendered at. Ticket 09 adds more.
const WINDOW: [f32; 2] = [900.0, 640.0];

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

/// What a text field currently holds, read out of the accessibility tree the way a screen
/// reader would — the shell exposes no accessor of its own (rule 12).
#[track_caller]
fn field_value(harness: &mut Harness<'_, InstallerApp>, label: &str) -> String {
    harness.get_by_label(label).value().unwrap_or_default()
}

#[track_caller]
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

/// The walking skeleton's demo, driven the way a user drives it, into the folders detection
/// found.
#[test]
fn clicking_install_deploys_the_community_patch() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));

    harness.step();
    assert!(
        harness.query_by_label("Ready.").is_some(),
        "the shell should start out idle",
    );

    set_text(
        &mut harness,
        "Community-Patch-DLL folder",
        &miniature_repo().display().to_string(),
    );
    harness.get_by_label("Install").click();
    wait_for_label(&mut harness, "Installed (1) Community Patch.");

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

/// User story 26: what the last run used pre-fills the next one, with nothing to detect.
#[test]
fn what_one_launch_settles_the_next_launch_starts_from() {
    let game = TempGame::new();
    let mut harness = harness_over(game.launch(&game.locations()));
    harness.step();
    set_text(
        &mut harness,
        "Community-Patch-DLL folder",
        &miniature_repo().display().to_string(),
    );
    harness.get_by_label("Install").click();
    wait_for_label(&mut harness, "Installed (1) Community Patch.");

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
    set_text(
        &mut harness,
        "Community-Patch-DLL folder",
        "/no/such/checkout",
    );

    harness.get_by_label("Install").click();
    wait_for_label(&mut harness, "There is no folder at");

    assert!(
        !game.mods_folder().join("(1) Community Patch").exists(),
        "nothing should have been written to the game",
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
