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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use civ5vp_core::GameFolders;
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

/// A game whose three folders exist, plus somewhere for the Core to work.
struct TempGame {
    temp: tempfile::TempDir,
    folders: GameFolders,
}

impl TempGame {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let folders = GameFolders {
            mods: temp.path().join("game/MODS"),
            dlc: temp.path().join("game/DLC"),
            text: temp.path().join("game/Text"),
        };
        for folder in [&folders.mods, &folders.dlc, &folders.text] {
            std::fs::create_dir_all(folder).unwrap();
        }
        Self { temp, folders }
    }

    fn core(&self) -> Arc<civ5vp_core::Core> {
        Arc::new(placeholder::core(self.temp.path().join("app-data")))
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

/// The walking skeleton's demo, driven the way a user drives it.
#[test]
fn clicking_install_deploys_the_community_patch() {
    let game = TempGame::new();
    let mut harness = harness_over(InstallerApp::with_paths(
        game.core(),
        &miniature_repo(),
        &game.folders,
    ));

    harness.step();
    assert!(
        harness.query_by_label("Ready.").is_some(),
        "the shell should start out idle",
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

    let deployed = game.folders.mods.join("(1) Community Patch");
    assert!(deployed.join("(1) Community Patch.modinfo").is_file());
    assert_eq!(
        std::fs::read_to_string(deployed.join("CvGameCore_Expansion2.dll")).unwrap(),
        placeholder::PLACEHOLDER_DLL_CONTENTS,
    );
}

/// A failure the user can act on, not a stack trace: the source folder does not exist.
#[test]
fn a_bad_source_folder_is_explained_and_nothing_is_installed() {
    let game = TempGame::new();
    let mut harness = harness_over(InstallerApp::with_paths(
        game.core(),
        &game.temp.path().join("no-such-checkout"),
        &game.folders,
    ));

    harness.get_by_label("Install").click();
    wait_for_label(&mut harness, "There is no folder at");

    assert!(
        !game.folders.mods.join("(1) Community Patch").exists(),
        "nothing should have been written to the game",
    );
}

/// Rule 6, from the outside: a MODS folder that is not a real absolute location is refused,
/// so Sync never aims its deletes at whatever the working directory happens to be.
#[test]
fn a_relative_mods_folder_is_refused_before_anything_is_written() {
    let game = TempGame::new();
    let folders = GameFolders {
        mods: PathBuf::from("MODS"),
        ..game.folders.clone()
    };
    let mut harness = harness_over(InstallerApp::with_paths(
        game.core(),
        &miniature_repo(),
        &folders,
    ));

    harness.get_by_label("Install").click();
    wait_for_label(&mut harness, "The MODS folder needs to be a full path");

    assert!(
        !Path::new("MODS").exists(),
        "a relative MODS folder must never be created next to the process",
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
