//! The full deployment matrix, asserted as file trees.
//!
//! The reference is the official InnoSetup script, `VPSetupData.iss` in
//! `LoneGazebo/Community-Patch-DLL`, which the spec names as the behavioural authority for
//! file placement. It models six mutually exclusive components; there is one test below per
//! component, named after it, and each asserts the *complete* resulting tree rather than
//! spot-checking a file. An exact list is what catches a folder that should not be there.
//!
//! The seventh combination — EUI with Community Patch only — has no test because it cannot be
//! written down: `Eui` is reachable only through `Flavor::VoxPopuli`, so the illegal
//! configuration is a compile error rather than a runtime rejection. `eui_is_unrepresentable_
//! with_community_patch_only` documents that, since a rule enforced by the type system is
//! invisible in a test suite otherwise.

mod support;

use civ5vp_core::{
    ClaimedFolder, Core, Eui, Flavor, FortyThreeCivs, GameFolders, InstallConfiguration,
    InstallationSource, ProgressReporter,
};
use support::{
    DLL_MARKER, FixtureSourceProvider, GameFixture, MarkerToolchainRunner, miniature_repo,
};

// The file groups the matrix is composed from. Each is what one Claimed Folder contributes,
// so a test body reads as its row of the table rather than as a wall of paths.

/// `(1) Community Patch` minus its top-level Lua. Note what is *not* here: the checked-in
/// `CvGameCore_Expansion2.dll` (ADR-0001 — the Built DLL below replaces it), the `.civ5proj`,
/// and `MANUAL INSTALL.txt`. Note what is: `Kit/`, which the official installer does ship.
///
/// `Core Files/LUA/CoreHelper.lua` is here on purpose and in every configuration. The EUI
/// strip removes the *top-level* `LUA` only — the official installer's exclusion is `\LUA`,
/// and the leading backslash anchors it to the source root. A nested `LUA` deeper in the tree
/// is ordinary mod content and must survive. Without this entry, making the exclusion
/// recursive would pass every test in this file.
const COMMUNITY_PATCH: &[&str] = &[
    "MODS/(1) Community Patch/(1) Community Patch.modinfo",
    "MODS/(1) Community Patch/Core Files/Core Values/DefinesChanges.sql",
    "MODS/(1) Community Patch/Core Files/LUA/CoreHelper.lua",
    "MODS/(1) Community Patch/CvGameCore_Expansion2.dll",
    "MODS/(1) Community Patch/Kit/ReadMe.txt",
];

/// `(1)`'s Lua, present only when EUI is off. With EUI on, `(3a)` supplies it instead.
const COMMUNITY_PATCH_LUA: &[&str] = &["MODS/(1) Community Patch/LUA/CityView.lua"];

/// `(2) Vox Populi` minus its Lua, and minus the `INSTRUCTIONS.txt` beside it.
const VOX_POPULI: &[&str] = &[
    "MODS/(2) Vox Populi/(2) Vox Populi.modinfo",
    "MODS/(2) Vox Populi/Balance Changes/BalanceChanges.sql",
];

const VOX_POPULI_LUA: &[&str] = &["MODS/(2) Vox Populi/LUA/PlotHelpManager.lua"];

const EUI_COMPATIBILITY_FILES: &[&str] = &[
    "MODS/(3a) VP - EUI Compatibility Files/(3a) VP - EUI Compatibility Files.modinfo",
    "MODS/(3a) VP - EUI Compatibility Files/EUI/NeededText.xml",
    "MODS/(3a) VP - EUI Compatibility Files/LUA/EUI_core_library.lua",
];

/// `(3b)` ships two files out of a folder that also holds a 43-civ DLL. That DLL stays behind:
/// the 43-civ build is deployed into `(1)`, which is where `(1)`'s modinfo looks for it.
const FORTY_THREE_CIVS: &[&str] = &[
    "MODS/(3b) 43 Civs Community Patch/(3b) 43 Civs Community Patch (v 1).modinfo",
    "MODS/(3b) 43 Civs Community Patch/AdvancedSetup.lua",
];

const SQUADS: &[&str] = &[
    "MODS/(4a) Squads for VP/(4a) Squads for VP.modinfo",
    "MODS/(4a) Squads for VP/UI/Squads.lua",
];

const VPUI_DLC: &[&str] = &["DLC/VPUI/FrontEnd/FrontEnd.lua", "DLC/VPUI/VPUI_0.Civ5Pkg"];

const UI_BC1_DLC: &[&str] = &[
    "DLC/UI_bc1/CityView/CityView.lua",
    "DLC/UI_bc1/EUI_0.Civ5Pkg",
];

/// The loading-screen tips, from `VPUI Text/` in the source — a different folder from the
/// `VPUI` deployed as DLC.
const TIPS: &[&str] = &["Text/VPUI_tips_en_us.xml"];

/// Flatten and sort the groups a configuration should produce.
fn expected(groups: &[&[&str]]) -> Vec<String> {
    let mut all: Vec<String> = groups
        .iter()
        .flat_map(|group| group.iter().map(|path| (*path).to_owned()))
        .collect();
    all.sort();
    all
}

fn configuration(flavor: Flavor, forty_three_civs: FortyThreeCivs) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: miniature_repo(),
        },
        flavor,
        forty_three_civs,
    }
}

fn core_over(game: &GameFixture) -> Core {
    Core::new(
        Box::new(FixtureSourceProvider::new(miniature_repo())),
        Box::new(MarkerToolchainRunner),
        game.work_dir(),
    )
}

/// Install `configuration` into a fresh game and return everything the game now contains.
fn install(game: &GameFixture, configuration: &InstallConfiguration) -> Vec<String> {
    let core = core_over(game);
    let plan = core
        .plan(configuration, &game.folders())
        .expect("this configuration is legal");
    core.execute(&plan, &ProgressReporter::silent())
        .expect("the install should succeed");
    game.files()
}

fn vox_populi(eui: Eui) -> Flavor {
    Flavor::VoxPopuli { eui }
}

/// InnoSetup component `Core`.
#[test]
fn community_patch_only() {
    let game = GameFixture::new();
    let files = install(
        &game,
        &configuration(Flavor::CommunityPatch, FortyThreeCivs::Disabled),
    );

    assert_eq!(files, expected(&[COMMUNITY_PATCH, COMMUNITY_PATCH_LUA]));
    // The deployed DLL is the one the toolchain built, never the repository's own.
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER,
    );
}

/// InnoSetup component `Civ43CPOnly`.
#[test]
fn community_patch_only_with_43_civs() {
    let game = GameFixture::new();
    let files = install(
        &game,
        &configuration(Flavor::CommunityPatch, FortyThreeCivs::Enabled),
    );

    assert_eq!(
        files,
        expected(&[COMMUNITY_PATCH, COMMUNITY_PATCH_LUA, FORTY_THREE_CIVS]),
    );
    // The 43-civ DLL goes into `(1)`, and it is still the only DLL anywhere in the game.
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER,
    );
}

/// InnoSetup component `FullNoEUI`.
#[test]
fn vox_populi_without_eui() {
    let game = GameFixture::new();
    let files = install(
        &game,
        &configuration(vox_populi(Eui::Disabled), FortyThreeCivs::Disabled),
    );

    assert_eq!(
        files,
        expected(&[
            COMMUNITY_PATCH,
            COMMUNITY_PATCH_LUA,
            VOX_POPULI,
            VOX_POPULI_LUA,
            SQUADS,
            VPUI_DLC,
            TIPS,
        ]),
    );
}

/// InnoSetup component `FullEUI`. The Lua in `(1)` and `(2)` is gone — `(3a)` replaces it.
#[test]
fn vox_populi_with_eui() {
    let game = GameFixture::new();
    let files = install(
        &game,
        &configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Disabled),
    );

    assert_eq!(
        files,
        expected(&[
            COMMUNITY_PATCH,
            VOX_POPULI,
            EUI_COMPATIBILITY_FILES,
            SQUADS,
            VPUI_DLC,
            UI_BC1_DLC,
            TIPS,
        ]),
    );
}

/// InnoSetup component `Civ43NoEUI`.
#[test]
fn vox_populi_without_eui_with_43_civs() {
    let game = GameFixture::new();
    let files = install(
        &game,
        &configuration(vox_populi(Eui::Disabled), FortyThreeCivs::Enabled),
    );

    assert_eq!(
        files,
        expected(&[
            COMMUNITY_PATCH,
            COMMUNITY_PATCH_LUA,
            VOX_POPULI,
            VOX_POPULI_LUA,
            FORTY_THREE_CIVS,
            SQUADS,
            VPUI_DLC,
            TIPS,
        ]),
    );
}

/// InnoSetup component `Civ43EUI` — every toggle on at once.
#[test]
fn vox_populi_with_eui_and_43_civs() {
    let game = GameFixture::new();
    let files = install(
        &game,
        &configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Enabled),
    );

    assert_eq!(
        files,
        expected(&[
            COMMUNITY_PATCH,
            VOX_POPULI,
            EUI_COMPATIBILITY_FILES,
            FORTY_THREE_CIVS,
            SQUADS,
            VPUI_DLC,
            UI_BC1_DLC,
            TIPS,
        ]),
    );
}

/// The one illegal combination, documented where a reader will look for it.
///
/// There is nothing to execute: `Eui` is reachable only through `Flavor::VoxPopuli`, so
/// "Community Patch with EUI" cannot be constructed. This test exists so that a later change
/// flattening `Flavor` into independent fields has to delete a test that says why not.
#[test]
fn eui_is_unrepresentable_with_community_patch_only() {
    let community_patch = Flavor::CommunityPatch;
    // There is no `eui` to set here. `Flavor::CommunityPatch { eui: Eui::Enabled }` does not
    // compile, which is the whole point — the check happens before the program runs.
    assert!(matches!(community_patch, Flavor::CommunityPatch));
    assert!(matches!(
        vox_populi(Eui::Enabled),
        Flavor::VoxPopuli { eui: Eui::Enabled },
    ));
}

/// User story 21: switching configurations removes exactly what no longer belongs.
///
/// This is the case that corrupts hand-made installs. Going from the largest configuration to
/// the smallest has to leave the game as if the smallest had been installed on a clean game —
/// no orphaned DLC folder, no leftover Lua, no stale tips file.
#[test]
fn switching_from_the_largest_configuration_to_the_smallest_converges() {
    let game = GameFixture::new();
    let core = core_over(&game);

    let everything = configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Enabled);
    let plan = core.plan(&everything, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let smallest = configuration(Flavor::CommunityPatch, FortyThreeCivs::Disabled);
    let plan = core.plan(&smallest, &game.folders()).unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(
        game.files(),
        expected(&[COMMUNITY_PATCH, COMMUNITY_PATCH_LUA]),
        "switching down should leave exactly a Community-Patch-only install",
    );
    assert_eq!(
        outcome.removed,
        vec![
            ClaimedFolder::VoxPopuli,
            ClaimedFolder::EuiCompatibilityFiles,
            ClaimedFolder::FortyThreeCivsCommunityPatch,
            ClaimedFolder::SquadsForVoxPopuli,
            ClaimedFolder::Vpui,
            ClaimedFolder::UiBc1,
        ],
        "everything the largest configuration added, and nothing else",
    );
}

/// The other direction, and the one that catches a Sync that merges instead of replacing:
/// turning EUI on has to *remove* the Lua that the previous configuration installed.
#[test]
fn turning_eui_on_removes_the_lua_it_replaces() {
    let game = GameFixture::new();
    let core = core_over(&game);

    let without = configuration(vox_populi(Eui::Disabled), FortyThreeCivs::Disabled);
    let plan = core.plan(&without, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert!(
        game.files()
            .contains(&"MODS/(1) Community Patch/LUA/CityView.lua".to_owned()),
    );

    let with = configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Disabled);
    let plan = core.plan(&with, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(
        game.files(),
        expected(&[
            COMMUNITY_PATCH,
            VOX_POPULI,
            EUI_COMPATIBILITY_FILES,
            SQUADS,
            VPUI_DLC,
            UI_BC1_DLC,
            TIPS,
        ]),
        "the Lua from the non-EUI install must be gone, not merged with (3a)'s",
    );
}

/// Rule 8, across the whole matrix: every configuration is idempotent.
#[test]
fn every_configuration_is_idempotent() {
    for (name, flavor, civs) in [
        ("Core", Flavor::CommunityPatch, FortyThreeCivs::Disabled),
        (
            "Civ43CPOnly",
            Flavor::CommunityPatch,
            FortyThreeCivs::Enabled,
        ),
        (
            "FullNoEUI",
            vox_populi(Eui::Disabled),
            FortyThreeCivs::Disabled,
        ),
        (
            "FullEUI",
            vox_populi(Eui::Enabled),
            FortyThreeCivs::Disabled,
        ),
        (
            "Civ43NoEUI",
            vox_populi(Eui::Disabled),
            FortyThreeCivs::Enabled,
        ),
        (
            "Civ43EUI",
            vox_populi(Eui::Enabled),
            FortyThreeCivs::Enabled,
        ),
    ] {
        let game = GameFixture::new();
        let core = core_over(&game);
        let plan = core
            .plan(&configuration(flavor, civs), &game.folders())
            .unwrap();

        core.execute(&plan, &ProgressReporter::silent()).unwrap();
        let after_first = game.files();
        core.execute(&plan, &ProgressReporter::silent()).unwrap();

        assert_eq!(game.files(), after_first, "{name} is not idempotent");
    }
}

/// Rule 6 across the whole matrix: the biggest configuration still touches nothing else.
#[test]
fn the_largest_configuration_leaves_unrelated_content_alone() {
    let game = GameFixture::new();
    game.plant("MODS/Some Other Mod/SomeOtherMod.modinfo", "not ours");
    game.plant("DLC/Expansion2/Expansion2.Civ5Pkg", "the game's own DLC");
    game.plant("Text/EN_US/SomeOtherText.xml", "another mod's text");
    game.plant("ModUserData/(1) Community Patch.db", "my saved settings");

    install(
        &game,
        &configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Enabled),
    );

    assert_eq!(
        game.read("MODS/Some Other Mod/SomeOtherMod.modinfo"),
        "not ours",
    );
    assert_eq!(
        game.read("DLC/Expansion2/Expansion2.Civ5Pkg"),
        "the game's own DLC",
    );
    assert_eq!(
        game.read("Text/EN_US/SomeOtherText.xml"),
        "another mod's text",
        "the Text Folder holds other mods' files and only the tips XML is ours",
    );
    assert_eq!(
        game.read("ModUserData/(1) Community Patch.db"),
        "my saved settings",
    );
}

/// Uninstalling after the largest configuration restores an unmodded game — including the
/// Claimed File in the Text Folder, which is the one that is easy to forget.
#[test]
fn uninstall_after_the_largest_configuration_restores_an_unmodded_game() {
    let game = GameFixture::new();
    game.plant("MODS/Some Other Mod/SomeOtherMod.modinfo", "not ours");
    game.plant("Text/EN_US/SomeOtherText.xml", "another mod's text");
    let unmodded = game.files();

    install(
        &game,
        &configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Enabled),
    );
    let core = core_over(&game);
    core.uninstall(&game.folders(), &ProgressReporter::silent())
        .expect("uninstall should succeed");

    assert_eq!(game.files(), unmodded);
}

/// A source folder the Plan needs but the Installation Source does not have is reported before
/// the game is touched, rather than producing a silently incomplete install.
#[test]
fn a_source_missing_a_needed_folder_is_reported_and_nothing_is_installed() {
    let game = GameFixture::new();
    let incomplete = game.work_dir().join("incomplete-source");
    std::fs::create_dir_all(incomplete.join("(1) Community Patch")).unwrap();
    std::fs::write(
        incomplete.join("(1) Community Patch/(1) Community Patch.modinfo"),
        "<Mod/>",
    )
    .unwrap();

    let core = Core::new(
        Box::new(FixtureSourceProvider::new(incomplete)),
        Box::new(MarkerToolchainRunner),
        game.work_dir(),
    );
    let plan = core
        .plan(
            &configuration(vox_populi(Eui::Disabled), FortyThreeCivs::Disabled),
            &game.folders(),
        )
        .unwrap();

    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a source without (2) Vox Populi cannot install it");

    assert!(
        error.user_message().contains("(2) Vox Populi"),
        "expected the missing folder named, got: {}",
        error.user_message(),
    );
    assert_eq!(game.files(), Vec::<String>::new(), "nothing was installed");
}

/// A source folder that is present but holds none of the files a configuration takes from it
/// is refused before Sync starts, not part-way through it (rule 7).
///
/// This is what upstream renaming `AdvancedSetup.lua` would look like. Deploying the empty
/// `(3b)` that would otherwise result reads as success and leaves the player with a mod that
/// silently does nothing.
#[test]
fn a_source_folder_with_none_of_the_files_it_should_have_is_refused_before_sync() {
    let game = GameFixture::new();
    game.plant(
        "MODS/(1) Community Patch/(1) Community Patch.modinfo",
        "mine",
    );
    let before = game.files();

    // A source that has everything except any of the two files `(3b)` ships.
    let source = game.work_dir().join("renamed-upstream");
    copy_dir(&miniature_repo(), &source);
    let slim = source.join("(3b) 43 Civs Community Patch");
    for entry in std::fs::read_dir(&slim).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            std::fs::remove_file(path).unwrap();
        }
    }

    let core = Core::new(
        Box::new(FixtureSourceProvider::new(source)),
        Box::new(MarkerToolchainRunner),
        game.work_dir(),
    );
    let plan = core
        .plan(
            &configuration(Flavor::CommunityPatch, FortyThreeCivs::Enabled),
            &game.folders(),
        )
        .unwrap();

    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a (3b) holding neither of its two files cannot be deployed");

    assert!(
        error
            .user_message()
            .contains("does not contain what this installer expects"),
        "expected a plain-language message, got: {}",
        error.user_message(),
    );
    assert_eq!(
        game.files(),
        before,
        "rule 7: the game must be untouched, including the folder Sync would have replaced",
    );
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let source = entry.unwrap().path();
        let destination = to.join(source.file_name().unwrap());
        if source.is_dir() {
            copy_dir(&source, &destination);
        } else {
            std::fs::copy(&source, &destination).unwrap();
        }
    }
}

/// The DLC folders are read from the source root, not from a `DLC` subdirectory — a mistake
/// that would silently produce an install with no user interface at all.
#[test]
fn the_dlc_folders_come_from_the_source_root() {
    let game = GameFixture::new();
    install(
        &game,
        &configuration(vox_populi(Eui::Enabled), FortyThreeCivs::Disabled),
    );

    assert_eq!(game.read("DLC/VPUI/VPUI_0.Civ5Pkg"), "VPUI package 0\n");
    assert_eq!(game.read("DLC/UI_bc1/EUI_0.Civ5Pkg"), "EUI package 0\n");
}

/// Sanity: the folders really are the ones the Core says it deployed, and a configuration that
/// deploys nothing to the DLC folder leaves it empty.
#[test]
fn community_patch_only_puts_nothing_in_the_dlc_or_text_folders() {
    let game = GameFixture::new();
    let folders: GameFolders = game.folders();
    install(
        &game,
        &configuration(Flavor::CommunityPatch, FortyThreeCivs::Disabled),
    );

    assert_eq!(std::fs::read_dir(&folders.dlc).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&folders.text).unwrap().count(), 0);
}
