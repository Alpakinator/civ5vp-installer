//! Core-seam tests for the Modpack install mode.
//!
//! Same house style as `deployment.rs`: fixture repository, temporary game folders, the
//! public Core API, assertions on the resulting file tree. The database engine is faked
//! ([`FixtureModpackAssembler`]) — what these tests pin down is everything around
//! it: what is staged, what crosses the seam, what Sync writes and removes, and the two
//! asymmetric removal rules the user asked for by name.

mod support;

use civ5vp_core::{
    BuildConfiguration, ClaimedFolder, Core, Eui, Flavor, FortyThreeCivs, InstallConfiguration,
    InstallError, InstallMode, InstallationSource, ProgressReporter,
};
use support::{
    DLL_MARKER, FixtureModpackAssembler, FixtureSourceProvider, GAMEPLAY_DUMP_MARKER, GameFixture,
    MarkerToolchainRunner, TEXT_DUMP_MARKER, miniature_repo,
};

fn vox_populi_modpack() -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: miniature_repo(),
        },
        flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Modpack,
        extra_mods: Vec::new(),
    }
}

fn mods_mode(mut configuration: InstallConfiguration) -> InstallConfiguration {
    configuration.install_mode = InstallMode::Mods;
    configuration
}

/// The game the Modpack tests run against: base UI files to copy, one GameData XML to
/// override, one XML that must not be, and a pristine cache to snapshot.
fn modpack_game() -> GameFixture {
    let game = GameFixture::new();
    game.plant(
        "DLC/Expansion2/UI/InGame/InGame.lua",
        "-- base InGame.lua\n",
    );
    game.plant(
        "DLC/Expansion2/UI/InGame/CityView/CityView.lua",
        "-- base CityView.lua\n",
    );
    game.plant(
        "DLC/Expansion2/UI/InGame/LeaderHead/LeaderHeadRoot.lua",
        "-- base LeaderHeadRoot.lua\n",
    );
    game.plant(
        "DLC/Expansion2/Gameplay/CIV5Fixture.xml",
        "<?xml version=\"1.0\"?>\n<GameData>\n</GameData>\n",
    );
    game.plant(
        "DLC/Expansion2/UI/SomeDialog.xml",
        "<?xml version=\"1.0\"?>\n<Context/>\n",
    );
    game.plant("cache/Civ5DebugDatabase.db", "pristine base");
    game.plant("cache/Localization-Merged.db", "pristine text");
    game
}

fn core_over(game: &GameFixture, assembler: FixtureModpackAssembler) -> Core {
    Core::new(
        Box::new(FixtureSourceProvider::new(miniature_repo())),
        Box::new(MarkerToolchainRunner),
        Box::new(assembler),
        game.work_dir(),
    )
}

/// The whole Modpack path at once: the pack is assembled and deployed, the databases cross
/// the seam in activation order, and — the user's rule — a Vox Populi install already
/// sitting in MODS is left exactly where it is.
#[test]
fn a_modpack_deployment_builds_the_pack_and_leaves_mods_alone() {
    let game = modpack_game();
    // A mod install from an earlier day. A Modpack Deployment must not touch it.
    game.plant("MODS/(2) Vox Populi/existing.txt", "left alone");
    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = core_over(&game, assembler);

    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    // The pack, in the game's DLC.
    assert!(
        game.read("DLC/VP_MODPACK/MPModsPack.Civ5Pkg")
            .contains("<GUID>{b5932ae4-0f4f-498f-9333-e2d31b20e095}</GUID>")
    );
    // The mods inside it, Built DLL included, repository clutter excluded.
    assert_eq!(
        game.read("DLC/VP_MODPACK/Mods/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER,
    );
    assert!(
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/(2) Vox Populi/Balance Changes/BalanceChanges.sql")
            .is_file()
    );
    assert!(
        !game
            .game_root()
            .join("DLC/VP_MODPACK/Mods/(1) Community Patch/MANUAL INSTALL.txt")
            .exists(),
        "the standard exclusions apply inside the pack too"
    );
    // The UI folder: base copies, the mod's own CityView.lua over the base one, and the
    // entry-point hook appended to InGame.lua. The mod's staged copy is deleted — two
    // files with one bare name collide in the game's VFS, and the hookless one could win.
    assert!(
        game.read("DLC/VP_MODPACK/UI/CityView.lua")
            .contains("print(\"CityView\")")
    );
    assert!(
        !game
            .game_root()
            .join("DLC/VP_MODPACK/Mods/(1) Community Patch/LUA/CityView.lua")
            .exists(),
        "the staged duplicate of a UI entry file must be deleted"
    );
    let in_game = game.read("DLC/VP_MODPACK/UI/InGame.lua");
    assert!(in_game.starts_with("-- base InGame.lua"));
    assert!(in_game.contains("g_uiAddins[#g_uiAddins + 1] = \"PlotHelpManager\";"));
    // The overrides: the game's GameData XML emptied by name, other XML untouched, and the
    // two dumps written by the assembler.
    assert_eq!(game.read("DLC/VP_MODPACK/Override/CIV5Fixture.xml"), "");
    assert!(
        !game
            .game_root()
            .join("DLC/VP_MODPACK/Override/SomeDialog.xml")
            .exists()
    );
    assert_eq!(
        game.read("DLC/VP_MODPACK/Override/CIV5Units.xml"),
        GAMEPLAY_DUMP_MARKER,
    );
    assert_eq!(
        game.read("DLC/VP_MODPACK/Override/CIV5Units_Mongol.xml"),
        TEXT_DUMP_MARKER,
    );
    // What crossed the seam: the two update files, in activation order.
    let jobs = jobs.lock().unwrap();
    let updates: Vec<String> = jobs[0]
        .updates
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(updates, ["DefinesChanges.sql", "BalanceChanges.sql"]);
    // The user's rule: MODS untouched, and not reported as removed.
    assert_eq!(game.read("MODS/(2) Vox Populi/existing.txt"), "left alone");
    assert_eq!(outcome.removed, Vec::new());
    // The mods went inside the pack, not into MODS.
    assert!(!game.game_root().join("MODS/(1) Community Patch").exists());
    // The DLC-target folders are still real DLC beside the pack.
    assert!(game.game_root().join("DLC/VPUI").is_dir());
    // The DLL's fingerprint sidecar lives beside the DLL inside the pack.
    assert!(
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/(1) Community Patch")
            .join("CvGameCore_Expansion2.dll.fingerprint")
            .is_file()
    );
    assert!(outcome.deployed.contains(&ClaimedFolder::Modpack));
    assert_eq!(
        outcome.built_dll,
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/(1) Community Patch/CvGameCore_Expansion2.dll"),
    );
}

/// A modinfo action referencing a file that is not there — which upstream really ships —
/// is skipped out loud, the way the game skips it, and the Modpack still builds.
#[test]
fn a_dangling_database_update_is_skipped_the_way_the_game_skips_it() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .expect("a dangling upstream reference must not make a Version un-Modpackable");

    assert!(game.game_root().join("DLC/VP_MODPACK").is_dir());
    let skipped: Vec<String> = receiver
        .try_iter()
        .filter(|event| event.message.contains("Skipped a database update"))
        .map(|event| event.message)
        .collect();
    assert!(
        skipped
            .iter()
            .any(|line| line.contains("RenamedLongAgo.xml") && line.contains("(1) Community Patch")),
        "the skip should name the mod and the dangling file, got: {skipped:?}"
    );
}

/// The other direction of the user's rule: a Mods-mode Deployment removes a Modpack,
/// because a baked-in Modpack loads at every startup and corrupts a mod-menu install.
#[test]
fn a_mods_deployment_removes_a_modpack() {
    let game = modpack_game();
    game.plant("DLC/VP_MODPACK/MPModsPack.Civ5Pkg", "an older modpack");
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(&mods_mode(vox_populi_modpack()), &game.folders())
        .unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(!game.game_root().join("DLC/VP_MODPACK").exists());
    assert!(outcome.removed.contains(&ClaimedFolder::Modpack));
    assert!(game.game_root().join("MODS/(2) Vox Populi").is_dir());
}

/// No pristine cache, no Modpack — and the game untouched, with a sentence telling the
/// player exactly what to do (launch the game unmodded once).
#[test]
fn a_modpack_deployment_refuses_a_modded_cache() {
    let game = modpack_game();
    game.plant(
        "cache/Civ5DebugDatabase.db",
        "modded by a session with mods",
    );
    let before = game.files();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a modded cache cannot seed a Modpack");

    assert!(matches!(error, InstallError::ModpackBaseUnavailable { .. }));
    assert!(error.user_message().contains("Start Civilization V"));
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
}

/// The snapshot outlives the cache: Sync clears the game's cache folder, and once the
/// Modpack is deployed every later launch would rebuild the cache with the Modpack baked
/// in — so an upgrade must run from the snapshot, and does.
#[test]
fn a_second_modpack_deployment_runs_from_the_snapshot() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert!(
        !game.game_root().join("cache/Civ5DebugDatabase.db").exists(),
        "Sync clears the game cache"
    );

    // No cache anywhere — only the snapshot can make this succeed.
    core.execute(&plan, &ProgressReporter::silent())
        .expect("the upgrade installs from the snapshot");
    assert!(game.game_root().join("DLC/VP_MODPACK").is_dir());
}

/// A MODS folder with the player's own mods in it, for the extra-mod tests.
fn plant_players_own_mods(game: &GameFixture) {
    game.plant(
        "MODS/Even More Bonuses/Even More Bonuses.modinfo",
        "<Mod id=\"11111111-2222-3333-4444-555555555555\" version=\"3\">\n\
         \t<Actions><OnModActivated>\n\
         \t\t<UpdateDatabase>Data/MoreBonuses.sql</UpdateDatabase>\n\
         \t</OnModActivated></Actions>\n\
         </Mod>",
    );
    game.plant(
        "MODS/Even More Bonuses/Data/MoreBonuses.sql",
        "UPDATE Defines SET Value = 2;",
    );
    // Not a mod: no modinfo, so it is never offered.
    game.plant("MODS/Screenshots/pretty.png", "not a mod");
    // The in-game Modpack Maker, excluded by its ID the way it excludes itself.
    game.plant(
        "MODS/(5) Modpack Maker for VP/(5) Modpack Maker for VP (v 1).modinfo",
        "<Mod id=\"eb8f6ed3-109d-4f2f-a81d-516c8d2f91c1\" version=\"1\"/>",
    );
}

/// What the extra-mod picker offers: modinfo-bearing folders only, minus the managed set
/// and the Modpack Maker.
#[test]
fn the_extra_mod_offer_lists_the_players_own_mods_only() {
    let game = modpack_game();
    plant_players_own_mods(&game);
    // A managed folder sitting in MODS from an earlier Mods-mode install: not offered,
    // the configuration governs it.
    game.plant(
        "MODS/(2) Vox Populi/(2) Vox Populi.modinfo",
        "<Mod id=\"x\"/>",
    );

    let offered = civ5vp_core::available_extra_mods(&game.folders().mods);

    assert_eq!(offered, vec!["Even More Bonuses".to_owned()]);
}

/// A picked extra mod is baked into the pack — copied inside, its database updates applied
/// after the managed set's — and its MODS original is left exactly where it was.
#[test]
fn a_picked_extra_mod_is_baked_in_after_the_managed_set() {
    let game = modpack_game();
    plant_players_own_mods(&game);
    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = core_over(&game, assembler);

    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = vec!["Even More Bonuses".to_owned()];
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/Even More Bonuses/Data/MoreBonuses.sql")
            .is_file(),
        "the pick is copied inside the pack"
    );
    let jobs = jobs.lock().unwrap();
    let updates: Vec<String> = jobs[0]
        .updates
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        updates,
        [
            "DefinesChanges.sql",
            "BalanceChanges.sql",
            "MoreBonuses.sql"
        ],
        "the modmod's changes land on top of Vox Populi's"
    );
    assert!(
        game.game_root()
            .join("MODS/Even More Bonuses/Data/MoreBonuses.sql")
            .is_file(),
        "the MODS original is only read"
    );
}

/// A pick that has since vanished stops the Deployment before the game is touched.
#[test]
fn a_vanished_extra_mod_stops_the_deployment() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = vec!["Even More Bonuses".to_owned()];
    let before = game.files();
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a vanished pick cannot be baked in");

    assert!(
        error.user_message().contains("Even More Bonuses"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
}

/// Uninstall treats the Modpack as what it is: a Claimed Folder.
#[test]
fn uninstall_removes_the_modpack() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    let outcome = core
        .uninstall(&game.folders(), &ProgressReporter::silent())
        .unwrap();

    assert!(!game.game_root().join("DLC/VP_MODPACK").exists());
    assert!(outcome.removed.contains(&ClaimedFolder::Modpack));
}
