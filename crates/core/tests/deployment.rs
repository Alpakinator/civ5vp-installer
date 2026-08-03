//! Core-seam tests: run an Install Configuration, assert the resulting file tree.

mod support;

use std::path::{Path, PathBuf};

use civ5vp_core::{
    ClaimedFolder, Core, Eui, Flavor, FortyThreeCivs, GameFolders, InstallConfiguration,
    InstallationSource, ProgressReporter, Stage,
};
use support::{
    DLL_MARKER, FailingSourceProvider, FailingToolchainRunner, FixtureSourceProvider, GameFixture,
    MarkerToolchainRunner, miniature_repo,
};

fn community_patch_only() -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: miniature_repo(),
        },
        flavor: Flavor::CommunityPatch,
        forty_three_civs: FortyThreeCivs::Disabled,
    }
}

fn core_over(game: &GameFixture) -> Core {
    Core::new(
        Box::new(FixtureSourceProvider::new(miniature_repo())),
        Box::new(MarkerToolchainRunner),
        game.work_dir(),
    )
}

/// The tracer bullet: one configuration, all the way through, reported back to the caller.
///
/// The resulting file tree is asserted in `matrix.rs`, which does it for all six legal
/// configurations. What is checked here is the part the shell renders — what the Core says it
/// did — and that the deployed DLL is the built one rather than the repository's stale copy
/// (ADR-0001).
#[test]
fn a_deployment_reports_what_it_did() {
    let game = GameFixture::new();
    let core = core_over(&game);

    let plan = core
        .plan(&community_patch_only(), &game.folders())
        .expect("Community Patch only is a legal configuration");
    let outcome = core
        .execute(&plan, &ProgressReporter::silent())
        .expect("the install should succeed");

    assert_eq!(outcome.deployed, vec![ClaimedFolder::CommunityPatch]);
    assert_eq!(outcome.removed, Vec::new());
    assert_eq!(
        outcome.built_dll,
        game.game_root()
            .join("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
    );
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER,
    );
}

/// Progress reaches the caller, in order, and the last word is about the game.
#[test]
fn progress_events_reach_the_caller() {
    let game = GameFixture::new();
    let core = core_over(&game);
    let (sender, receiver) = std::sync::mpsc::channel();

    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .unwrap();

    let stages: Vec<Stage> = receiver.iter().map(|event| event.stage).collect();
    assert!(!stages.is_empty(), "expected progress events");
    assert_eq!(stages.first(), Some(&Stage::Fetch));
    assert_eq!(stages.last(), Some(&Stage::Sync));
    assert!(
        stages
            .windows(2)
            .all(|pair| stage_order(pair[0]) <= stage_order(pair[1])),
        "stages should never go backwards: {stages:?}",
    );
}

fn stage_order(stage: Stage) -> u8 {
    match stage {
        Stage::Fetch => 0,
        Stage::Build => 1,
        Stage::Sync => 2,
    }
}

/// Running the same configuration again converges on the same tree, and anything that crept
/// into a Claimed Folder in between is gone.
#[test]
fn a_second_run_converges_on_the_same_tree() {
    let game = GameFixture::new();
    let core = core_over(&game);
    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();

    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    let after_first = game.files();

    game.plant(
        "MODS/(1) Community Patch/LeftoverFromAnOlderVersion.sql",
        "stale",
    );
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(game.files(), after_first);
}

/// Rule 6: only the Claimed Folders are ever touched.
#[test]
fn content_outside_the_claimed_folders_survives() {
    let game = GameFixture::new();
    let core = core_over(&game);
    game.plant("MODS/Some Other Mod/SomeOtherMod.modinfo", "not ours");
    game.plant("DLC/Expansion2/Expansion2.Civ5Pkg", "the game's own DLC");

    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(
        game.read("MODS/Some Other Mod/SomeOtherMod.modinfo"),
        "not ours"
    );
    assert_eq!(
        game.read("DLC/Expansion2/Expansion2.Civ5Pkg"),
        "the game's own DLC"
    );
}

/// User story 23: the stale-cache corruption the community fixes by hand. The game's `cache`
/// folder is emptied after Deployment, and `ModUserData` — its sibling — is left alone.
#[test]
fn the_game_cache_is_cleared_and_mod_user_data_is_preserved() {
    let game = GameFixture::new();
    let core = core_over(&game);
    game.plant(
        "cache/Civ5DebugDatabase.db",
        "stale cache from the last install",
    );
    game.plant("cache/Localization-Merged.db", "also stale");
    game.plant(
        "ModUserData/(1) Community Patch.db",
        "my saved mod settings",
    );

    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(
        !game.game_root().join("cache/Civ5DebugDatabase.db").exists(),
        "the cache should have been cleared",
    );
    assert!(
        !game
            .game_root()
            .join("cache/Localization-Merged.db")
            .exists(),
        "the cache should have been cleared",
    );
    assert!(
        game.game_root().join("cache").is_dir(),
        "the cache folder itself should survive — the game expects to find it",
    );
    assert_eq!(
        game.read("ModUserData/(1) Community Patch.db"),
        "my saved mod settings",
        "ModUserData is never touched",
    );
}

/// User story 24: Uninstall returns an unmodded game.
#[test]
fn uninstall_removes_every_claimed_folder_and_leaves_everything_else() {
    let game = GameFixture::new();
    let core = core_over(&game);
    game.plant("MODS/Some Other Mod/SomeOtherMod.modinfo", "not ours");
    game.plant("DLC/Expansion2/Expansion2.Civ5Pkg", "the game's own DLC");
    game.plant(
        "ModUserData/(1) Community Patch.db",
        "my saved mod settings",
    );
    let unmodded = game.files();

    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    game.plant("cache/Civ5DebugDatabase.db", "written by the game since");
    assert_ne!(
        game.files(),
        unmodded,
        "the install should have changed something"
    );

    let outcome = core
        .uninstall(&game.folders(), &ProgressReporter::silent())
        .expect("uninstall should succeed");

    assert_eq!(
        game.files(),
        unmodded,
        "the game should be back to unmodded"
    );
    assert_eq!(outcome.removed, vec![ClaimedFolder::CommunityPatch]);
}

/// Uninstalling a game that was never modded is a no-op rather than an error, so the button
/// is safe to press twice.
#[test]
fn uninstall_is_idempotent() {
    let game = GameFixture::new();
    let core = core_over(&game);
    game.plant("MODS/Some Other Mod/SomeOtherMod.modinfo", "not ours");
    let before = game.files();

    for _ in 0..2 {
        let outcome = core
            .uninstall(&game.folders(), &ProgressReporter::silent())
            .expect("uninstalling an unmodded game should succeed");
        assert_eq!(outcome.removed, Vec::new());
        assert_eq!(game.files(), before);
    }
}

/// Uninstall deletes things, so it runs the same folder checks a Deployment does — otherwise
/// a relative MODS folder would aim `remove_dir_all` at the working directory (rule 6).
#[test]
fn uninstall_refuses_game_folders_it_cannot_trust() {
    let game = GameFixture::new();
    let core = core_over(&game);
    let folders = GameFolders {
        mods: PathBuf::from("MODS"),
        ..game.folders()
    };

    let error = core
        .uninstall(&folders, &ProgressReporter::silent())
        .expect_err("a relative MODS folder should be refused");

    assert!(
        error.user_message().contains("full path"),
        "expected a plain-language message, got: {}",
        error.user_message(),
    );
    assert!(!Path::new("MODS").exists());
}

/// The `cache` folder Sync clears is the MODS and Text Folders' sibling. If those two do not
/// agree on where the game is, there is no single right answer — so the configuration is
/// refused rather than a `cache` somewhere being emptied on a guess.
#[test]
fn game_folders_that_disagree_about_where_the_game_is_are_refused() {
    let game = GameFixture::new();
    let core = core_over(&game);
    let elsewhere = game.game_root().join("somewhere-else");
    std::fs::create_dir_all(elsewhere.join("Text")).unwrap();

    let folders = GameFolders {
        text: elsewhere.join("Text"),
        ..game.folders()
    };
    let error = core
        .plan(&community_patch_only(), &folders)
        .expect_err("MODS and Text in different places should be refused");

    assert!(
        error
            .user_message()
            .contains("same \"Sid Meier's Civilization 5\" folder"),
        "expected a plain-language message, got: {}",
        error.user_message(),
    );
}

/// Rule 7: a failed fetch aborts before the game is touched.
#[test]
fn a_failed_fetch_leaves_the_game_untouched() {
    let game = GameFixture::new();
    game.plant(
        "MODS/(1) Community Patch/(1) Community Patch.modinfo",
        "the install I am playing",
    );
    let before = game.files();

    let core = Core::new(
        Box::new(FailingSourceProvider),
        Box::new(MarkerToolchainRunner),
        game.work_dir(),
    );
    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("the fetch should fail");

    assert_eq!(game.files(), before);
    assert!(
        error.user_message().contains("internet connection"),
        "expected a plain-language message, got: {}",
        error.user_message(),
    );
}

/// Rule 7 again, one stage later: a failed build must not touch the game either.
#[test]
fn a_failed_build_leaves_the_game_untouched() {
    let game = GameFixture::new();
    game.plant(
        "MODS/(1) Community Patch/(1) Community Patch.modinfo",
        "the install I am playing",
    );
    let before = game.files();

    let core = Core::new(
        Box::new(FixtureSourceProvider::new(miniature_repo())),
        Box::new(FailingToolchainRunner),
        game.work_dir(),
    );
    let plan = core.plan(&community_patch_only(), &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("the build should fail");

    assert_eq!(game.files(), before);
    // Rule 10: a build failure suggests picking a Release.
    assert!(
        error.user_message().contains("Release"),
        "expected the Release suggestion, got: {}",
        error.user_message(),
    );
}

/// Rule 6: Sync derives every path it writes from a game folder root, so a root that is not
/// a real absolute location would send its deletes at the working directory. Each such folder
/// is refused at plan time, before any fetching, building, or writing.
#[test]
fn game_folders_that_are_not_real_absolute_directories_are_refused() {
    let game = GameFixture::new();
    let core = core_over(&game);
    let good = game.folders();

    let cases = [
        (
            "relative",
            GameFolders {
                mods: PathBuf::from("MODS"),
                ..good.clone()
            },
            "full path",
        ),
        (
            "empty",
            GameFolders {
                dlc: PathBuf::new(),
                ..good.clone()
            },
            "Choose your DLC folder",
        ),
        (
            "missing",
            GameFolders {
                text: good.text.join("does-not-exist"),
                ..good.clone()
            },
            "There is no Text folder at",
        ),
    ];

    for (name, folders, expected) in cases {
        let error = core
            .plan(&community_patch_only(), &folders)
            .err()
            .unwrap_or_else(|| panic!("the {name} case should be refused"));
        assert!(
            error.user_message().contains(expected),
            "{name}: expected a message mentioning {expected:?}, got: {}",
            error.user_message(),
        );
    }

    assert_eq!(game.files(), Vec::<String>::new());
    assert!(
        !Path::new("MODS").exists(),
        "a relative MODS folder must never be created next to the test process",
    );
}

/// Vox Populi used to be refused at plan time — the walking skeleton could only deploy the
/// Community Patch. It plans now. What the whole matrix actually deploys is `matrix.rs`.
#[test]
fn vox_populi_is_a_legal_configuration() {
    let game = GameFixture::new();
    let core = core_over(&game);

    let configuration = InstallConfiguration {
        flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
        ..community_patch_only()
    };

    core.plan(&configuration, &game.folders())
        .expect("Vox Populi with EUI is a legal configuration");
}
