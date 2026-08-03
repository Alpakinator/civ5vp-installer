//! How the shell is seen and driven.
//!
//! Two things are checked here, and they are the two halves of "the UI works":
//!
//! * behaviour — the AccessKit tree is queried by label and clicked, exactly as a screen
//!   reader would, and the install that follows is asserted on disk;
//! * looks — every screen is rendered to a committed PNG baseline, so a later change to the
//!   theme shows up as a visual diff rather than as nothing at all (rule 15).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use civ5vp_core::GameFolders;
use civ5vp_installer::{InstallerApp, Screen, Status, placeholder};
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

/// The walking skeleton's demo, driven the way a user drives it.
#[test]
fn clicking_install_deploys_the_community_patch() {
    let temp = tempfile::tempdir().unwrap();
    let folders = GameFolders {
        mods: temp.path().join("game/MODS"),
        dlc: temp.path().join("game/DLC"),
        text: temp.path().join("game/Text"),
    };
    for folder in [&folders.mods, &folders.dlc, &folders.text] {
        std::fs::create_dir_all(folder).unwrap();
    }

    let core = Arc::new(placeholder::core(temp.path().join("app-data")));
    let mut harness = harness_over(InstallerApp::with_paths(core, &miniature_repo(), &folders));

    assert_eq!(harness.state().status(), &Status::Ready);
    harness.get_by_label("Install").click();

    // The install runs on a worker thread, so step the UI until it reports back.
    for _ in 0..200 {
        harness.step();
        if harness.state().status() != &Status::Installing {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        matches!(harness.state().status(), Status::Installed { .. }),
        "expected the install to finish, got {:?} — activity: {:?}",
        harness.state().status(),
        harness.state().activity(),
    );
    assert!(
        harness
            .state()
            .status_line()
            .contains("(1) Community Patch"),
        "the shell should name what it installed, got {:?}",
        harness.state().status_line(),
    );
    assert!(
        !harness.state().activity().is_empty(),
        "progress from the Core should have reached the shell",
    );

    let deployed = folders.mods.join("(1) Community Patch");
    assert!(deployed.join("(1) Community Patch.modinfo").is_file());
    assert_eq!(
        std::fs::read_to_string(deployed.join("CvGameCore_Expansion2.dll")).unwrap(),
        placeholder::PLACEHOLDER_DLL_CONTENTS,
    );
}

/// A failure the user can act on, not a stack trace: the source folder does not exist.
#[test]
fn a_bad_source_folder_is_explained_and_nothing_is_installed() {
    let temp = tempfile::tempdir().unwrap();
    let folders = GameFolders {
        mods: temp.path().join("game/MODS"),
        dlc: temp.path().join("game/DLC"),
        text: temp.path().join("game/Text"),
    };
    std::fs::create_dir_all(&folders.mods).unwrap();

    let core = Arc::new(placeholder::core(temp.path().join("app-data")));
    let mut harness = harness_over(InstallerApp::with_paths(
        core,
        &temp.path().join("no-such-checkout"),
        &folders,
    ));

    harness.get_by_label("Install").click();
    for _ in 0..200 {
        harness.step();
        if harness.state().status() != &Status::Installing {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let Status::Failed { message } = harness.state().status() else {
        panic!("expected a failure, got {:?}", harness.state().status());
    };
    assert!(
        message.contains("no folder at"),
        "expected a plain-language message, got {message:?}",
    );
    assert!(
        !folders.mods.join("(1) Community Patch").exists(),
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
