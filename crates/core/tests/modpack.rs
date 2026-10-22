//! Core-seam tests for the Modpack install mode.
//!
//! Same house style as `deployment.rs`: fixture repository, temporary game folders, the
//! public Core API, assertions on the resulting file tree. The database engine is faked
//! ([`FixtureModpackAssembler`]) - what these tests pin down is everything around
//! it: what is staged, what crosses the seam, what Sync writes and removes, and the two
//! asymmetric removal rules the user asked for by name.

mod support;

use civ5vp_core::{
    BuildConfiguration, ClaimedFolder, Core, DllSource, Eui, Flavor, FortyThreeCivs,
    InstallConfiguration, InstallError, InstallMode, InstallationSource, LuaJitEngine,
    ProgressReporter,
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
        luajit: LuaJitEngine::Stock,
        menu_theme: civ5vp_core::MenuTheme::Stock,
        opening_movie: civ5vp_core::OpeningMovie::Stock,
        dll_source: DllSource::ShippedWhenCurrent,
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

/// Assembling the pack is reported as its own stage, after the DLL's and before the game is
/// touched. Never under the DLL's: a Modpack built on a Release compiles nothing, and its
/// lines must not read as a DLL build.
#[test]
fn modpack_assembly_is_reported_as_its_own_stage() {
    use civ5vp_core::Stage;
    let game = modpack_game();
    let (assembler, _) = FixtureModpackAssembler::new();
    let core = core_over(&game, assembler);
    let (sender, receiver) = std::sync::mpsc::channel();

    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .unwrap();

    let events: Vec<_> = receiver.iter().collect();
    let order = |stage: Stage| match stage {
        Stage::Fetch => 0,
        Stage::Build => 1,
        Stage::Modpack => 2,
        Stage::Sync => 3,
    };
    assert!(
        events
            .windows(2)
            .all(|pair| order(pair[0].stage) <= order(pair[1].stage)),
        "stages should never go backwards: {events:?}",
    );
    let assembly = events
        .iter()
        .find(|event| event.message == "Assembling the Modpack.")
        .expect("the assembly is reported");
    assert_eq!(assembly.stage, Stage::Modpack);
    assert!(!Stage::Modpack.label().contains("DLL"));
}

/// The whole Modpack path at once: the pack is assembled and deployed, the databases cross
/// the seam in activation order, and - the user's rule - a Vox Populi install already
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
    // entry-point hook appended to InGame.lua. The mod's staged copy is deleted - two
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
    // What crossed the seam: the Text Folder input, then mod updates in activation order.
    let jobs = jobs.lock().unwrap();
    let updates: Vec<String> = jobs[0]
        .updates
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        updates,
        [
            "VPUI_tips_en_us.xml",
            "DefinesChanges.sql",
            "BalanceChanges.sql"
        ]
    );
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
    // No map script to list, so nothing is put in Maps or Text for one.
    assert!(!game.game_root().join("Maps/VP_MODPACK").exists());
    assert!(
        !game
            .game_root()
            .join("Text/VP_MODPACK_MapScripts.xml")
            .exists()
    );
    assert!(!outcome.deployed.contains(&ClaimedFolder::ModpackMaps));
    assert_eq!(
        outcome.built_dll,
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/(1) Community Patch/CvGameCore_Expansion2.dll"),
    );
}

/// A modinfo action referencing a file that is not there - which upstream really ships -
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

/// The Vox Populi intro is not part of the pack: the game finds its opening movie by a fixed
/// path in the Game Installation. A Modpack Deployment has to put it there all the same.
#[test]
fn a_modpack_deployment_installs_the_vox_populi_intro_too() {
    let game = modpack_game();
    let movie = "Assets/DLC/Expansion2/Civ5XP2_Opening_Movie_en_US.wmv";
    game.plant(movie, "Brave New World's own opening movie");
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let configuration = InstallConfiguration {
        opening_movie: civ5vp_core::OpeningMovie::VoxPopuli,
        ..vox_populi_modpack()
    };
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    // The WMV on Windows, the WebM Proton plays everywhere else (ADR-0009).
    let encoding = if cfg!(windows) { "wmv" } else { "webm" };
    let intro = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../assets/intro/Civ5XP2_Opening_Movie_en_US.{encoding}"
        )),
    )
    .unwrap();
    assert!(std::fs::read(game.game_root().join(movie)).unwrap() == intro);
}

/// No pristine cache, no Modpack - and the game untouched, with a sentence telling the
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
/// in - so an upgrade must run from the snapshot, and does.
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

    // No cache anywhere - only the snapshot can make this succeed.
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

    let offer = civ5vp_core::extra_mod_offer(&game.folders().mods);

    assert_eq!(offer.offered, vec!["Even More Bonuses".to_owned()]);
    // Neither the picture folder nor the Modpack Maker is a mod to explain away either.
    assert_eq!(offer.refused, Vec::new());
}

/// A mod whose `.modinfo` sits a folder down - the shape a download unpacked into a folder
/// of its own takes - is a mod to the game, so it is offered and baked in like any other:
/// its database updates and UI hooks resolve from the folder that holds the `.modinfo`.
#[test]
fn a_mod_nested_inside_its_folder_is_offered_and_baked_in() {
    let game = modpack_game();
    game.plant(
        "MODS/Trade_Scanner_v25/Trade Scanner (v 25)/Trade Scanner (v 25).modinfo",
        "<Mod id=\"trade-scanner\" version=\"25\">\n\
         \t<Actions><OnModActivated>\n\
         \t\t<UpdateDatabase>Data/ScannerText.sql</UpdateDatabase>\n\
         \t</OnModActivated></Actions>\n\
         \t<EntryPoints><EntryPoint type=\"InGameUIAddin\" file=\"UI/TradeScanner.xml\"/></EntryPoints>\n\
         </Mod>",
    );
    game.plant(
        "MODS/Trade_Scanner_v25/Trade Scanner (v 25)/Data/ScannerText.sql",
        "-- fixture",
    );
    game.plant(
        "MODS/Trade_Scanner_v25/Trade Scanner (v 25)/UI/TradeScanner.xml",
        "<Context/>",
    );

    let offered = civ5vp_core::extra_mod_offer(&game.folders().mods).offered;
    assert_eq!(offered, vec!["Trade_Scanner_v25".to_owned()]);

    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = core_over(&game, assembler);
    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = offered;
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let jobs = jobs.lock().unwrap();
    assert!(
        jobs[0]
            .updates
            .iter()
            .any(|path| path.ends_with("Trade Scanner (v 25)/Data/ScannerText.sql")),
        "{:?}",
        jobs[0].updates
    );
    assert!(
        game.read("DLC/VP_MODPACK/UI/InGame.lua")
            .contains("\"TradeScanner\""),
        "the nested mod's UI addin is hooked"
    );
}

/// Only a mod the pack can carry whole is offered: a player must never believe a mod is in
/// their Modpack when part of it is not. A Modpack has no Mods menu, so an entry point the
/// pack does not hook, or a file the modinfo names but the mod lacks, keeps it from being
/// picked. It is still listed, after the ones that can be, with the reason in a player's
/// words - a mod that silently is not there reads as a bug.
#[test]
fn a_mod_the_pack_cannot_carry_whole_is_listed_as_refused_with_the_reason() {
    let game = modpack_game();
    plant_players_own_mods(&game);
    // A custom game-setup screen, opened from the Mods menu a Modpack game never shows.
    game.plant(
        "MODS/Giant Earth/Giant Earth.modinfo",
        "<Mod id=\"giant-earth\" version=\"1\">\n\
         \t<EntryPoints><EntryPoint type=\"Custom\" file=\"GameSetup/Setup.xml\"/></EntryPoints>\n\
         </Mod>",
    );
    game.plant("MODS/Giant Earth/GameSetup/Setup.xml", "<Context/>");
    // A database update the mod does not carry.
    game.plant(
        "MODS/Half A Mod/Half A Mod.modinfo",
        "<Mod id=\"half\" version=\"1\"><Actions><OnModActivated>\
         <UpdateDatabase>Gone.sql</UpdateDatabase></OnModActivated></Actions></Mod>",
    );
    // A minimap overlay: its hook target exists only if a mod brings MiniMapPanel.lua.
    game.plant(
        "MODS/Overlay/Overlay.modinfo",
        "<Mod id=\"overlay\" version=\"1\"><EntryPoints>\
         <EntryPoint type=\"MiniMapOverlayAddin\" file=\"Overlay.lua\"/></EntryPoints></Mod>",
    );
    game.plant("MODS/Overlay/Overlay.lua", "-- fixture");
    // Its own game DLL: the game loads one, and in a Modpack that is the Built DLL.
    game.plant(
        "MODS/Own DLL Mod/Own DLL Mod.modinfo",
        "<Mod id=\"own-dll\" version=\"1\"/>",
    );
    game.plant("MODS/Own DLL Mod/CvGameCore_Expansion2.dll", "MZ");
    // The same overlay bringing the file it hooks into is fine.
    game.plant(
        "MODS/Overlay With Panel/Overlay.modinfo",
        "<Mod id=\"overlay-panel\" version=\"1\"><EntryPoints>\
         <EntryPoint type=\"MiniMapOverlayAddin\" file=\"Overlay.lua\"/></EntryPoints></Mod>",
    );
    game.plant("MODS/Overlay With Panel/Overlay.lua", "-- fixture");
    game.plant("MODS/Overlay With Panel/UI/MiniMapPanel.lua", "-- fixture");

    let offer = civ5vp_core::extra_mod_offer(&game.folders().mods);

    assert_eq!(
        offer.offered,
        vec![
            "Even More Bonuses".to_owned(),
            "Overlay With Panel".to_owned()
        ]
    );
    let refused: Vec<(&str, &str)> = offer
        .refused
        .iter()
        .map(|refused| (refused.name.as_str(), refused.reason.as_str()))
        .collect();
    assert_eq!(
        refused,
        vec![
            (
                "Giant Earth",
                "part of it (GameSetup/Setup.xml) only runs when the mod is activated in the \
                 Mods menu"
            ),
            (
                "Half A Mod",
                "its .modinfo lists \"Gone.sql\", which is not in the mod"
            ),
            (
                "Overlay",
                "part of it (Overlay.lua) plugs into MiniMapPanel.lua, which nothing in the \
                 modpack has"
            ),
            (
                "Own DLL Mod",
                "it brings its own game DLL, and a modpack runs Vox Populi's"
            ),
        ]
    );
}

/// A pick that can no longer be carried whole - edited since it was offered - stops the
/// build before the game is touched, rather than baking in part of it.
#[test]
fn a_pick_the_pack_cannot_carry_whole_stops_the_deployment() {
    let game = modpack_game();
    plant_players_own_mods(&game);
    game.plant(
        "MODS/Even More Bonuses/Even More Bonuses.modinfo",
        "<Mod id=\"11111111-2222-3333-4444-555555555555\" version=\"3\">\n\
         \t<Actions><OnModActivated>\n\
         \t\t<UpdateDatabase>Data/MoreBonuses.sql</UpdateDatabase>\n\
         \t\t<UpdateDatabase>Data/AddedLater.sql</UpdateDatabase>\n\
         \t</OnModActivated></Actions>\n\
         </Mod>",
    );
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = vec!["Even More Bonuses".to_owned()];
    let before = game.files();
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("half a mod must not be baked in");

    let message = error.user_message();
    assert!(
        message.contains("Even More Bonuses") && message.contains("Data/AddedLater.sql"),
        "got: {message}"
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
}

/// A picked extra mod is baked into the pack - copied inside, its database updates applied
/// after the managed set's - and its MODS original is left exactly where it was.
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
            "VPUI_tips_en_us.xml",
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
/// A map-script mod: one script of its own, which builds its name from a text key while
/// the game scans it, and one named like a script of the game's own map pack.
fn plant_map_script_mod(game: &GameFixture) {
    game.plant(
        "MODS/Tectonic Map Script (v 6)/Tectonic Map Script (v 6).modinfo",
        "<Mod id=\"tectonic\" version=\"6\"><EntryPoints>\
         <EntryPoint type=\"MapScript\" file=\"Maps/Tectonic.lua\"/>\
         <EntryPoint type=\"MapScript\" file=\"Maps/Amazon_XP.lua\"/>\
         </EntryPoints></Mod>",
    );
    game.plant(
        "MODS/Tectonic Map Script (v 6)/Maps/Tectonic.lua",
        "function GetMapScriptInfo()\n\treturn { Name = L(\"TXT_KEY_TECTONIC\") .. \" - v6\", \
         Description = \"TXT_KEY_TECTONIC_HELP\" }\nend\n",
    );
    game.plant(
        "MODS/Tectonic Map Script (v 6)/Maps/Amazon_XP.lua",
        "-- overrides the game's own Amazon_XP.lua by name\n",
    );
    game.plant("DLC/Expansion/Maps/Amazon_XP.lua", "-- the game's own\n");
}

fn modpack_with(extra_mods: &[&str]) -> InstallConfiguration {
    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = extra_mods.iter().map(|name| (*name).to_owned()).collect();
    configuration
}

/// A Modpack is not registered DLC and activates no mods, so the game lists none of its map
/// scripts on its own - except those named like one of the game's, which override that file
/// in place. The rest go to the Documents-side Maps folder, and the text they look up while
/// the game scans them, before the pack's text is loaded, to the Text folder (ADR-0008).
#[test]
fn a_modpack_lists_its_map_scripts_where_the_game_finds_them() {
    let game = modpack_game();
    plant_map_script_mod(&game);
    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = core_over(&game, assembler);

    assert!(
        civ5vp_core::extra_mod_offer(&game.folders().mods)
            .offered
            .contains(&"Tectonic Map Script (v 6)".to_owned()),
        "a mod carrying its map scripts bakes in whole"
    );
    let plan = core
        .plan(
            &modpack_with(&["Tectonic Map Script (v 6)"]),
            &game.folders(),
        )
        .unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(
        game.read("Maps/VP_MODPACK/Tectonic.lua")
            .contains("TXT_KEY_TECTONIC")
    );
    assert!(
        !game
            .game_root()
            .join("Maps/VP_MODPACK/Amazon_XP.lua")
            .exists(),
        "a script named like the game's own is listed through it already"
    );
    assert!(
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/Tectonic Map Script (v 6)/Maps/Amazon_XP.lua")
            .is_file(),
        "the pack still carries it, which is what overrides the game's copy"
    );
    assert_eq!(
        jobs.lock().unwrap()[0].text_tags,
        ["TXT_KEY_TECTONIC", "TXT_KEY_TECTONIC_HELP"]
    );
    let text = game.read("Text/VP_MODPACK_MapScripts.xml");
    assert!(text.contains("<GameData>"), "got: {text}");
    assert!(
        text.contains(
            "\t<Language_en_US>\n\t\t<Replace Tag=\"TXT_KEY_TECTONIC\">\n\
             \t\t\t<Text>TXT_KEY_TECTONIC &amp; text</Text>\n\t\t</Replace>\n"
        ),
        "got: {text}"
    );
    assert!(outcome.deployed.contains(&ClaimedFolder::ModpackMaps));
    assert!(
        outcome
            .deployed_files
            .contains(&civ5vp_core::ClaimedFile::ModpackMapScriptText)
    );
}

/// The map scripts belong to the Modpack: a Mods Deployment takes them away with it, and
/// leaves the player's own maps beside them alone.
#[test]
fn a_mods_deployment_removes_the_modpack_map_scripts() {
    let game = modpack_game();
    plant_map_script_mod(&game);
    game.plant("Maps/My Own Map.Civ5Map", "the player's");
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let modpack = modpack_with(&["Tectonic Map Script (v 6)"]);
    let plan = core.plan(&modpack, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert!(
        game.game_root()
            .join("Maps/VP_MODPACK/Tectonic.lua")
            .is_file()
    );

    let plan = core.plan(&mods_mode(modpack), &game.folders()).unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(!game.game_root().join("Maps/VP_MODPACK").exists());
    assert!(
        !game
            .game_root()
            .join("Text/VP_MODPACK_MapScripts.xml")
            .exists()
    );
    assert!(outcome.removed.contains(&ClaimedFolder::ModpackMaps));
    assert_eq!(game.read("Maps/My Own Map.Civ5Map"), "the player's");
}

/// A map script the player already has in Maps under the same name stays the one the game
/// lists - the pack's copy is not put beside it.
#[test]
fn a_players_own_map_script_of_the_same_name_stays_listed() {
    let game = modpack_game();
    plant_map_script_mod(&game);
    game.plant("Maps/tectonic.lua", "-- the player's own copy");
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(
            &modpack_with(&["Tectonic Map Script (v 6)"]),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(
        !game
            .game_root()
            .join("Maps/VP_MODPACK/Tectonic.lua")
            .exists()
    );
    assert_eq!(game.read("Maps/tectonic.lua"), "-- the player's own copy");
}

#[test]
fn uninstall_removes_the_modpack_map_scripts() {
    let game = modpack_game();
    plant_map_script_mod(&game);
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let plan = core
        .plan(
            &modpack_with(&["Tectonic Map Script (v 6)"]),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let outcome = core
        .uninstall(&game.folders(), &ProgressReporter::silent())
        .unwrap();

    assert!(!game.game_root().join("Maps/VP_MODPACK").exists());
    assert!(
        !game
            .game_root()
            .join("Text/VP_MODPACK_MapScripts.xml")
            .exists()
    );
    assert!(outcome.removed.contains(&ClaimedFolder::ModpackMaps));
    assert!(
        outcome
            .removed_files
            .contains(&civ5vp_core::ClaimedFile::ModpackMapScriptText)
    );
}

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

#[test]
fn commented_and_inactive_manifest_actions_do_not_enter_the_modpack() {
    let game = modpack_game();
    game.plant(
        "MODS/An extra mod/Extra.modinfo",
        "<Mod id=\"extra\" version=\"1\">
         <!-- <Actions><OnModActivated><UpdateDatabase>Disabled.sql</UpdateDatabase></OnModActivated></Actions> -->
         <Actions>
           <OnModActivated><UpdateDatabase>Active.sql</UpdateDatabase></OnModActivated>
           <OnModDeactivated><UpdateDatabase>Cleanup.sql</UpdateDatabase></OnModDeactivated>
         </Actions>
         <EntryPoints>
           <!-- <EntryPoint type=\"InGameUIAddin\" file=\"DisabledPanel.lua\"/> -->
           <EntryPoint type=\"InGameUIAddin\" file=\"ActivePanel.lua\"/>
         </EntryPoints></Mod>",
    );
    for file in [
        "Active.sql",
        "Disabled.sql",
        "Cleanup.sql",
        "ActivePanel.lua",
        "DisabledPanel.lua",
    ] {
        game.plant(&format!("MODS/An extra mod/{file}"), "-- fixture");
    }
    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = core_over(&game, assembler);
    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = vec!["An extra mod".to_owned()];
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    let jobs = jobs.lock().unwrap();
    let names: Vec<_> = jobs[0]
        .updates
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy())
        .collect();
    assert!(names.iter().any(|n| n == "Active.sql"));
    assert!(
        !names
            .iter()
            .any(|n| n == "Disabled.sql" || n == "Cleanup.sql"),
        "{names:?}"
    );
    let ui = game.read("DLC/VP_MODPACK/UI/InGame.lua");
    assert!(ui.contains("\"ActivePanel\""));
    assert!(!ui.contains("DisabledPanel"));
}

#[test]
fn a_differently_cased_ui_override_is_the_one_that_receives_addin_hooks() {
    let game = modpack_game();
    game.plant(
        "MODS/An extra mod/Extra.modinfo",
        "<Mod id=\"extra\" version=\"1\"/>",
    );
    game.plant(
        "MODS/An extra mod/ingame.lua",
        "-- extra mod InGame override\n",
    );
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let mut configuration = vox_populi_modpack();
    configuration.extra_mods = vec!["An extra mod".to_owned()];
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    let ui = game.read("DLC/VP_MODPACK/UI/InGame.lua");
    assert!(ui.starts_with("-- extra mod InGame override"));
    assert!(ui.contains("\"PlotHelpManager\""));
    let ui_files = std::fs::read_dir(game.folders().dlc.join("VP_MODPACK/UI")).unwrap();
    assert_eq!(
        ui_files
            .filter(|e| e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("InGame.lua"))
            .count(),
        1
    );
    assert!(
        !game
            .folders()
            .dlc
            .join("VP_MODPACK/Mods/An extra mod/ingame.lua")
            .exists()
    );
}

#[test]
fn extra_mods_are_distinct_folder_names_not_paths() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    for names in [
        vec!["../outside"],
        vec!["..\\outside"],
        vec!["/tmp/outside"],
        vec!["C:\\outside"],
        vec!["."],
        vec![".."],
        vec![""],
        vec!["An extra mod", "An extra mod"],
        vec!["Extra", "extra"],
    ] {
        let mut configuration = vox_populi_modpack();
        configuration.extra_mods = names.iter().map(|s| (*s).to_owned()).collect();
        assert!(
            core.plan(&configuration, &game.folders()).is_err(),
            "accepted {names:?}"
        );
    }
}

#[test]
fn non_xml_unicode_asset_names_do_not_crash_modpack_assembly() {
    let game = modpack_game();
    game.plant("DLC/Expansion2/图图", "not GameData");
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert_eq!(game.read("DLC/Expansion2/图图"), "not GameData");
}
