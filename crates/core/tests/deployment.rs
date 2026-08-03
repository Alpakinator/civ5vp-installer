//! Core-seam tests: run an Install Configuration, assert the resulting file tree.

mod support;

use civ5vp_core::{
    ClaimedFolder, Core, Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallationSource,
    ProgressReporter, Stage,
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

/// The tracer bullet: one configuration, all the way through, asserted on disk.
#[test]
fn community_patch_only_deploys_one_mod_folder_and_the_built_dll() {
    let game = GameFixture::new();
    let core = core_over(&game);

    let plan = core
        .plan(&community_patch_only(), &game.folders())
        .expect("Community Patch only is a legal configuration");
    let outcome = core
        .execute(&plan, &ProgressReporter::silent())
        .expect("the install should succeed");

    assert_eq!(
        game.files(),
        vec![
            "MODS/(1) Community Patch/(1) Community Patch.modinfo",
            "MODS/(1) Community Patch/Core Files/Core Values/DefinesChanges.sql",
            "MODS/(1) Community Patch/CvGameCore_Expansion2.dll",
            "MODS/(1) Community Patch/LUA/CityView.lua",
        ],
    );

    // The deployed DLL is the one the toolchain built, not the stale one checked into the
    // repository (ADR-0001).
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER,
    );
    assert_eq!(outcome.deployed, vec![ClaimedFolder::CommunityPatch]);
    assert_eq!(outcome.removed, Vec::new());
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

/// The walking skeleton cannot deploy Vox Populi yet. It says so at plan time, before any
/// fetching or building happens — ticket 02 replaces this with the real matrix.
#[test]
fn vox_populi_is_refused_at_plan_time_for_now() {
    let game = GameFixture::new();
    let core = core_over(&game);

    let configuration = InstallConfiguration {
        flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
        ..community_patch_only()
    };
    let error = core
        .plan(&configuration, &game.folders())
        .expect_err("Vox Populi is not deployable in the walking skeleton");

    assert!(
        error.user_message().contains("Vox Populi"),
        "expected a plain-language message, got: {}",
        error.user_message(),
    );
    assert_eq!(game.files(), Vec::<String>::new());
}
