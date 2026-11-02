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
    assert!(in_game.starts_with("-- fixture: the Community Patch's own InGame.lua"));
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
        Some(
            game.game_root()
                .join("DLC/VP_MODPACK/Mods/(1) Community Patch/CvGameCore_Expansion2.dll")
        ),
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

/// Sync clears the cache, so a cache that exists while a Modpack is deployed was written by a
/// launch with that pack in it, whatever the database says: a pack of only the player's own
/// mods leaves no Community Patch table behind and activates no mod. Here no launch of the game
/// can help, so the sentence has to say to uninstall first.
#[test]
fn a_cache_written_with_a_modpack_deployed_cannot_be_the_base() {
    let game = modpack_game();
    game.plant("DLC/VP_MODPACK/MPModsPack.Civ5Pkg", "a deployed pack");
    let before = game.files();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core.plan(&vox_populi_modpack(), &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("the cache has the deployed Modpack in it");

    assert!(
        error.user_message().contains("Uninstall"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
    assert!(
        !game.work_dir().join("modpack-base").exists(),
        "it must not be kept as the base of later builds"
    );
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

    let offer = civ5vp_core::extra_mod_offer(&game.folders().mods, &vox_populi_modpack());

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

    let offered = civ5vp_core::extra_mod_offer(&game.folders().mods, &vox_populi_modpack()).offered;
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
    game.plant(
        "MODS/Overlay With Panel/UI/MiniMapPanel.lua",
        PANEL_TAKING_HOOKS,
    );

    // A pack of the player's own mods: no Community Patch to bring a MiniMapPanel.lua.
    let offer = civ5vp_core::extra_mod_offer(&game.folders().mods, &own_mods_only_modpack());

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
                "part of it (Overlay.lua) plugs into a MiniMapPanel.lua that takes hooks, \
                 which none of the selected mods has"
            ),
            (
                "Own DLL Mod",
                "it brings its own game DLL, and a modpack cannot carry one from a mod"
            ),
        ]
    );
}

/// A MiniMapPanel.lua that does something with a hook line: the Community Patch's shape,
/// where assigning into `g_uiAddins` loads the context.
const PANEL_TAKING_HOOKS: &str = "-- a whole MiniMapPanel\n\
    local g_uiAddins = {};\n\
    setmetatable(g_uiAddins, {\n\
    \t__newindex = function(_, _, value)\n\
    \t\tContextPtr:LoadNewContext(value);\n\
    \tend,\n\
    });\n";

/// Three of the player's mods for the cross-mod tests: a minimap overlay and a map-generation
/// hook, neither bringing the file it plugs into, and a mod that brings MiniMapPanel.lua.
fn plant_hooks_and_a_provider(game: &GameFixture) {
    game.plant(
        "MODS/Overlay/Overlay.modinfo",
        "<Mod id=\"overlay\" version=\"1\"><EntryPoints>\
         <EntryPoint type=\"MiniMapOverlayAddin\" file=\"Overlay.lua\"/></EntryPoints></Mod>",
    );
    game.plant("MODS/Overlay/Overlay.lua", "-- fixture");
    game.plant(
        "MODS/Map Hook/Map Hook.modinfo",
        "<Mod id=\"map-hook\" version=\"1\"><EntryPoints>\
         <EntryPoint type=\"PreMapGenScript\" file=\"Hook.lua\"/></EntryPoints></Mod>",
    );
    game.plant("MODS/Map Hook/Hook.lua", "-- fixture");
    game.plant(
        "MODS/Panel Provider/Panel Provider.modinfo",
        "<Mod id=\"panel-provider\" version=\"1\"/>",
    );
    game.plant(
        "MODS/Panel Provider/Deep/Inside/minimappanel.lua",
        PANEL_TAKING_HOOKS,
    );
}

/// Names of the offered mods, and of the refused ones with the file each one misses.
fn offer_for(
    game: &GameFixture,
    configuration: &InstallConfiguration,
) -> (Vec<String>, Vec<String>) {
    let offer = civ5vp_core::extra_mod_offer(&game.folders().mods, configuration);
    let refused = offer
        .refused
        .iter()
        .map(|refused| format!("{}: {}", refused.name, refused.reason))
        .collect();
    (offer.offered, refused)
}

/// A hook target counts wherever in the selection it comes from - the Community Patch, or
/// any ticked mod, at any depth and in any case, the way the pack copies it into `UI/` - and
/// the offer follows the ticks: ticking a provider makes the mod that needs it offerable,
/// and unticking it takes that away again.
#[test]
fn a_hook_target_counts_wherever_in_the_selection_it_comes_from() {
    let game = modpack_game();
    plant_hooks_and_a_provider(&game);
    let all = ["Map Hook", "Overlay", "Panel Provider"]
        .map(str::to_owned)
        .to_vec();

    // The Community Patch brings both MiniMapPanel.lua and MapGenerator.lua.
    let (offered, refused) = offer_for(&game, &modpack_with(&[]));
    assert_eq!(offered, all);
    assert_eq!(refused, Vec::<String>::new());

    // Without it, nothing selected brings either - until the provider is ticked.
    let own_mods = |ticked: &[&str]| InstallConfiguration {
        extra_mods: ticked.iter().map(|name| (*name).to_owned()).collect(),
        ..own_mods_only_modpack()
    };
    let (offered, refused) = offer_for(&game, &own_mods(&[]));
    assert_eq!(offered, ["Panel Provider"]);
    assert_eq!(
        refused,
        [
            "Map Hook: part of it (Hook.lua) plugs into a MapGenerator.lua that takes hooks, \
             which none of the selected mods has",
            "Overlay: part of it (Overlay.lua) plugs into a MiniMapPanel.lua that takes hooks, \
             which none of the selected mods has",
        ]
    );

    let (offered, _) = offer_for(&game, &own_mods(&["Panel Provider"]));
    assert_eq!(offered, ["Overlay", "Panel Provider"]);
    let (offered, _) = offer_for(&game, &own_mods(&["Overlay", "Panel Provider"]));
    assert_eq!(offered, ["Overlay", "Panel Provider"]);

    // The provider unticked: the overlay, still ticked, can no longer be carried whole.
    let (offered, refused) = offer_for(&game, &own_mods(&["Overlay"]));
    assert_eq!(offered, ["Panel Provider"]);
    assert!(
        refused.iter().any(|line| line.starts_with("Overlay:")),
        "{refused:?}"
    );
}

/// Only a ticked mod the pack can carry whole provides anything: one refused for a reason of
/// its own brings nothing into the pack, so nothing may lean on it.
#[test]
fn a_refused_mod_provides_nothing() {
    let game = modpack_game();
    plant_hooks_and_a_provider(&game);
    game.plant(
        "MODS/Panel Provider/Panel Provider.modinfo",
        "<Mod id=\"panel-provider\" version=\"1\"><Actions><OnModActivated>\
         <UpdateDatabase>Gone.sql</UpdateDatabase></OnModActivated></Actions></Mod>",
    );

    let configuration = InstallConfiguration {
        extra_mods: vec!["Overlay".to_owned(), "Panel Provider".to_owned()],
        ..own_mods_only_modpack()
    };
    let (offered, _) = offer_for(&game, &configuration);

    assert!(!offered.contains(&"Overlay".to_owned()), "{offered:?}");
}

/// The build goes by the same rule as the offer, against what is really staged: a pick whose
/// hook target nothing selected brings stops it before the game is touched, and the same pick
/// beside a provider is baked in with its hook in place.
#[test]
fn the_build_checks_hook_targets_against_the_whole_selection() {
    let game = modpack_game();
    plant_hooks_and_a_provider(&game);
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let alone = InstallConfiguration {
        extra_mods: vec!["Overlay".to_owned()],
        ..own_mods_only_modpack()
    };
    let before = game.files();
    let plan = core.plan(&alone, &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("an overlay with nothing to hook into cannot be baked in whole");
    assert!(
        error.user_message().contains("MiniMapPanel.lua"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");

    let with_provider = InstallConfiguration {
        extra_mods: vec!["Overlay".to_owned(), "Panel Provider".to_owned()],
        ..own_mods_only_modpack()
    };
    let plan = core.plan(&with_provider, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    let panel = game.read("DLC/VP_MODPACK/UI/MiniMapPanel.lua");
    assert!(panel.starts_with("-- a whole MiniMapPanel"));
    assert!(panel.contains("g_uiAddins[#g_uiAddins + 1] = \"Overlay\";"));
}

/// With Vox Populi the Community Patch brings the hook targets, so the overlay and the
/// map-generation hook are baked in on their own, hooked into its copies.
#[test]
fn the_community_patch_brings_the_hook_targets_for_the_players_mods() {
    let game = modpack_game();
    plant_hooks_and_a_provider(&game);
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(&modpack_with(&["Map Hook", "Overlay"]), &game.folders())
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert!(
        game.read("DLC/VP_MODPACK/UI/MiniMapPanel.lua")
            .contains("g_uiAddins[#g_uiAddins + 1] = \"Overlay\";")
    );
    assert!(
        game.read("DLC/VP_MODPACK/UI/MapGenerator.lua")
            .contains("g_uiAddins[#g_uiAddins + 1] = \"Hook\";")
    );
}

/// A mod with one UI addin, for the tests of how a hook reaches the file it goes into.
fn plant_ui_addin(game: &GameFixture) {
    game.plant(
        "MODS/Addin/Addin.modinfo",
        "<Mod id=\"addin\" version=\"1\"><EntryPoints>\
         <EntryPoint type=\"InGameUIAddin\" file=\"Addin.xml\"/></EntryPoints></Mod>",
    );
    game.plant("MODS/Addin/Addin.xml", "<Context/>");
}

const LOADED: &str = "ContextPtr:LoadNewContext(\"Addin\");";
const LISTED: &str = "g_uiAddins[#g_uiAddins + 1] = \"Addin\";";

/// The game's own InGame.lua has a plain `g_uiAddins` table, so a line assigning into it would
/// load nothing: the hook has to load the context itself.
#[test]
fn a_ui_hook_in_the_games_own_file_loads_its_context() {
    let game = modpack_game();
    plant_ui_addin(&game);
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(
            &InstallConfiguration {
                extra_mods: vec!["Addin".to_owned()],
                ..own_mods_only_modpack()
            },
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let in_game = game.read("DLC/VP_MODPACK/UI/InGame.lua");
    assert!(in_game.ends_with(&format!("\n{LOADED}\n")), "{in_game}");
    assert!(!in_game.contains(LISTED), "{in_game}");
}

/// The Community Patch's InGame.lua loads whatever is assigned into `g_uiAddins`, so packs with
/// it keep the line they always had, to the byte.
#[test]
fn a_ui_hook_in_the_community_patchs_file_keeps_its_line() {
    let game = modpack_game();
    plant_ui_addin(&game);
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(&modpack_with(&["Addin"]), &game.folders())
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let in_game = game.read("DLC/VP_MODPACK/UI/InGame.lua");
    assert!(in_game.ends_with(&format!("\n{LISTED}\n")), "{in_game}");
    assert!(!in_game.contains(LOADED), "{in_game}");
}

/// It is the file that ends up in the pack that decides: a mod's own InGame.lua replacing the
/// Community Patch's, without its table, gets the line that loads the context.
#[test]
fn a_ui_hook_follows_a_mods_override_of_the_file() {
    let game = modpack_game();
    plant_ui_addin(&game);
    game.plant("MODS/Addin/InGame.lua", "-- a mod's plain InGame.lua\n");
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(&modpack_with(&["Addin"]), &game.folders())
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let in_game = game.read("DLC/VP_MODPACK/UI/InGame.lua");
    assert!(in_game.starts_with("-- a mod's plain InGame.lua"));
    assert!(in_game.contains(LOADED), "{in_game}");
    assert!(!in_game.contains(LISTED), "{in_game}");
}

/// MapGenerator.lua runs its list later and has no UI context to load anything into: a hook
/// there is a line in the list, whatever the file looks like.
#[test]
fn a_map_generation_hook_is_listed_and_never_loaded_as_a_context() {
    let game = modpack_game();
    plant_hooks_and_a_provider(&game);
    game.plant(
        "MODS/Generator Provider/Generator Provider.modinfo",
        "<Mod id=\"generator-provider\" version=\"1\"/>",
    );
    game.plant(
        "MODS/Generator Provider/MapGenerator.lua",
        "local g_uiAddins = {};\nfor _, file in ipairs( g_uiAddins ) do include(file); end\n",
    );
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(
            &InstallConfiguration {
                extra_mods: vec!["Generator Provider".to_owned(), "Map Hook".to_owned()],
                ..own_mods_only_modpack()
            },
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let generator = game.read("DLC/VP_MODPACK/UI/MapGenerator.lua");
    assert!(generator.contains("g_uiAddins[#g_uiAddins + 1] = \"Hook\";"));
    assert!(!generator.contains("LoadNewContext"), "{generator}");
}

/// Sharing the name is not enough. A MiniMapPanel.lua or MapGenerator.lua that does nothing with
/// a hook line would leave the mod plugged into nothing, so it provides nothing - in the offer,
/// and in the build, which goes by the same rule.
#[test]
fn a_file_that_takes_no_hooks_provides_nothing() {
    let game = modpack_game();
    plant_hooks_and_a_provider(&game);
    game.plant(
        "MODS/Panel Provider/Deep/Inside/minimappanel.lua",
        "-- the game's plain MiniMapPanel\n",
    );
    game.plant(
        "MODS/Panel Provider/MapGenerator.lua",
        "local g_uiAddins = {};\n",
    );
    let picks = InstallConfiguration {
        extra_mods: vec![
            "Map Hook".to_owned(),
            "Overlay".to_owned(),
            "Panel Provider".to_owned(),
        ],
        ..own_mods_only_modpack()
    };

    let (offered, refused) = offer_for(&game, &picks);
    assert_eq!(offered, ["Panel Provider"]);
    assert_eq!(refused.len(), 2, "{refused:?}");

    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let before = game.files();
    let plan = core.plan(&picks, &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("nothing in the pack takes these hooks");
    assert!(
        error.user_message().contains("takes hooks"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
}

const COMMUNITY_PATCH_ID: &str = "d1b6328c-ff44-4b0d-aad7-c657f83610cd";
const VOX_POPULI_ID: &str = "8411a7a8-dad3-4622-a18e-fcc18324c799";
const EUI_COMPATIBILITY_ID: &str = "24923240-e4fb-4bf6-8f0e-6e5b6cf4d3c2";
const FORTY_THREE_CIVS_ID: &str = "6521c02b-1c78-4826-a5b0-faed86235e98";
const SQUADS_ID: &str = "3645dbca-bdfb-4d86-bdc5-46cb5a426cc2";

/// A mod in MODS with these `<Dependencies>`, written the way upstream writes them.
fn plant_mod_needing(game: &GameFixture, folder: &str, id: &str, needs: &[(&str, &str)]) {
    let needs: String = needs
        .iter()
        .map(|(id, title)| {
            format!("<Mod id=\"{id}\" minversion=\"17\" maxversion=\"999\" title=\"{title}\" />")
        })
        .collect();
    game.plant(
        &format!("MODS/{folder}/{folder}.modinfo"),
        &format!("<Mod id=\"{id}\" version=\"1\"><Dependencies>{needs}</Dependencies></Mod>"),
    );
}

fn with_flavor(flavor: Flavor, ticked: &[&str]) -> InstallConfiguration {
    InstallConfiguration {
        flavor,
        ..modpack_with(ticked)
    }
}

fn neither(ticked: &[&str]) -> InstallConfiguration {
    InstallConfiguration {
        extra_mods: ticked.iter().map(|name| (*name).to_owned()).collect(),
        ..own_mods_only_modpack()
    }
}

/// A mod made for Vox Populi not installing without Vox Populi is right, and the installer does
/// not try to make it work: it only must not be tickable. The reason says what is missing.
#[test]
fn a_mod_is_refused_while_the_mods_it_depends_on_are_not_selected() {
    let game = modpack_game();
    plant_mod_needing(
        &game,
        "For VP",
        "for-vp",
        &[
            (COMMUNITY_PATCH_ID, "(1) Community Patch"),
            (VOX_POPULI_ID, "(2) Vox Populi"),
        ],
    );

    let (offered, refused) = offer_for(&game, &neither(&[]));
    assert_eq!(offered, Vec::<String>::new());
    assert_eq!(refused, ["For VP: it needs Community Patch and Vox Populi"]);

    let (offered, refused) = offer_for(&game, &with_flavor(Flavor::CommunityPatch, &[]));
    assert_eq!(offered, Vec::<String>::new());
    assert_eq!(refused, ["For VP: it needs Vox Populi"]);

    let (offered, refused) = offer_for(&game, &modpack_with(&[]));
    assert_eq!(offered, ["For VP"]);
    assert_eq!(refused, Vec::<String>::new());
}

/// Each managed mod counts by its real ID, exactly while the configuration deploys it.
#[test]
fn the_managed_mods_a_configuration_deploys_satisfy_dependencies() {
    let game = modpack_game();
    plant_mod_needing(
        &game,
        "Needs EUI",
        "needs-eui",
        &[(EUI_COMPATIBILITY_ID, "(3a) VP - EUI Compatibility Files")],
    );
    plant_mod_needing(
        &game,
        "Needs 43 Civs",
        "needs-43",
        &[(FORTY_THREE_CIVS_ID, "(3b) 43 Civs Community Patch")],
    );
    plant_mod_needing(
        &game,
        "Needs Squads",
        "needs-squads",
        &[(SQUADS_ID, "(4a) Squads for VP")],
    );
    let offered = |configuration: &InstallConfiguration| offer_for(&game, configuration).0;

    assert_eq!(
        offered(&with_flavor(Flavor::CommunityPatch, &[])),
        Vec::<String>::new()
    );
    assert_eq!(offered(&modpack_with(&[])), ["Needs Squads"]);
    assert_eq!(
        offered(&with_flavor(Flavor::VoxPopuli { eui: Eui::Enabled }, &[])),
        ["Needs EUI", "Needs Squads"]
    );
    let forty_three = FortyThreeCivs::Enabled;
    assert_eq!(
        offered(&InstallConfiguration {
            forty_three_civs: forty_three,
            ..with_flavor(Flavor::CommunityPatch, &[])
        }),
        ["Needs 43 Civs"]
    );
    // 43 Civs is ignored without the Community Patch: it deploys nothing there.
    assert_eq!(
        offered(&InstallConfiguration {
            forty_three_civs: forty_three,
            ..neither(&[])
        }),
        Vec::<String>::new()
    );
}

/// A ticked mod satisfies a dependency like a managed one, but only a mod that is itself kept:
/// one refused for want of Vox Populi carries nothing for the mod leaning on it.
#[test]
fn a_dependency_on_another_of_the_players_mods_follows_the_ticks() {
    let game = modpack_game();
    plant_mod_needing(
        &game,
        "Promotion Tree",
        "promotion-tree",
        &[(VOX_POPULI_ID, "(2) Vox Populi")],
    );
    plant_mod_needing(
        &game,
        "Tree Addon",
        // Written with the case the game ignores.
        "tree-addon",
        &[("PROMOTION-TREE", "UI - Promotion Tree for VP")],
    );

    let (offered, refused) = offer_for(&game, &modpack_with(&[]));
    assert_eq!(offered, ["Promotion Tree"]);
    assert_eq!(refused, ["Tree Addon: it needs UI - Promotion Tree for VP"]);

    let (offered, _) = offer_for(&game, &modpack_with(&["Promotion Tree"]));
    assert_eq!(offered, ["Promotion Tree", "Tree Addon"]);

    let (offered, refused) = offer_for(&game, &neither(&["Promotion Tree", "Tree Addon"]));
    assert_eq!(offered, Vec::<String>::new());
    assert_eq!(
        refused,
        [
            "Promotion Tree: it needs Vox Populi",
            "Tree Addon: it needs UI - Promotion Tree for VP",
        ]
    );
}

/// Two mods unpacked into one folder are picked together, so one can depend on the other.
#[test]
fn a_dependency_inside_the_mods_own_folder_is_satisfied() {
    let game = modpack_game();
    game.plant(
        "MODS/Bundle/Base/Base.modinfo",
        "<Mod id=\"bundle-base\" version=\"1\"/>",
    );
    game.plant(
        "MODS/Bundle/Addon/Addon.modinfo",
        "<Mod id=\"bundle-addon\" version=\"1\"><Dependencies>\
         <Mod id=\"bundle-base\" title=\"Base\" /></Dependencies></Mod>",
    );

    assert_eq!(offer_for(&game, &neither(&[])).0, ["Bundle"]);
}

/// What the game asks for is what counts: a soft `<References>`, a DLC or a commented-out
/// dependency is no reason to refuse a mod, and a dependency with no title still says what it is.
#[test]
fn only_a_real_dependency_on_a_mod_refuses_one() {
    let game = modpack_game();
    game.plant(
        "MODS/Soft/Soft.modinfo",
        "<Mod id=\"soft\" version=\"1\"><Dependencies>\
         <!-- <Mod id=\"gone\" title=\"Gone\" /> -->\
         <Dlc id=\"7459BA32-5764-44ae-8E95-01AD0E0EFD48\" minversion=\"0\" maxversion=\"999\" />\
         </Dependencies><References><Mod id=\"other\" title=\"Other\" /></References>\
         <Blocks><Mod id=\"another\" title=\"Another\" /></Blocks></Mod>",
    );
    game.plant(
        "MODS/Empty/Empty.modinfo",
        "<Mod id=\"empty\" version=\"1\"><Dependencies /></Mod>",
    );
    game.plant(
        "MODS/Untitled/Untitled.modinfo",
        "<Mod id=\"untitled\" version=\"1\"><Dependencies>\
         <Mod id=\"abc-123\" /></Dependencies></Mod>",
    );

    let (offered, refused) = offer_for(&game, &neither(&[]));
    assert_eq!(offered, ["Empty", "Soft"]);
    assert_eq!(refused, ["Untitled: it needs the mod abc-123"]);
}

/// The build goes by the same rule against the modinfos really staged, and stops before the
/// game is touched.
#[test]
fn the_build_checks_dependencies_against_the_whole_selection() {
    let game = modpack_game();
    plant_mod_needing(
        &game,
        "For VP",
        "for-vp",
        &[(VOX_POPULI_ID, "(2) Vox Populi")],
    );
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let before = game.files();
    let plan = core.plan(&neither(&["For VP"]), &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a mod for Vox Populi cannot go into a pack without it");
    assert!(
        error.user_message().contains("it needs Vox Populi"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");

    let plan = core
        .plan(&modpack_with(&["For VP"]), &game.folders())
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert!(
        game.game_root()
            .join("DLC/VP_MODPACK/Mods/For VP/For VP.modinfo")
            .exists()
    );
}

/// The `(3b)` modinfo is staged even though most of that folder stays behind, and `(3a)` comes
/// with EUI: both count at build time by the IDs in the files.
#[test]
fn the_build_finds_the_managed_mods_by_the_ids_in_their_modinfos() {
    let game = modpack_game();
    plant_mod_needing(
        &game,
        "For Everything",
        "for-everything",
        &[
            (EUI_COMPATIBILITY_ID, "(3a) VP - EUI Compatibility Files"),
            (FORTY_THREE_CIVS_ID, "(3b) 43 Civs Community Patch"),
            (SQUADS_ID, "(4a) Squads for VP"),
        ],
    );
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let configuration = InstallConfiguration {
        forty_three_civs: FortyThreeCivs::Enabled,
        ..with_flavor(Flavor::VoxPopuli { eui: Eui::Enabled }, &["For Everything"])
    };

    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
}

/// A mod in MODS with these `<Blocks>`, written the way upstream writes them.
fn plant_mod_blocking(game: &GameFixture, folder: &str, id: &str, blocks: &[(&str, &str)]) {
    let blocks: String = blocks
        .iter()
        .map(|(id, title)| {
            format!("<Mod id=\"{id}\" minversion=\"0\" maxversion=\"999\" title=\"{title}\" />")
        })
        .collect();
    game.plant(
        &format!("MODS/{folder}/{folder}.modinfo"),
        &format!("<Mod id=\"{id}\" version=\"1\"><Blocks>{blocks}</Blocks></Mod>"),
    );
}

/// The game enforces `<Blocks>` in its Mods menu, and a Modpack activates nothing, so here
/// nothing else would. Either side may declare it. The mod the player has not ticked is the
/// one greyed out, and it comes back when the other is unticked.
#[test]
fn a_mod_blocked_by_something_selected_is_refused_until_that_is_unticked() {
    let game = modpack_game();
    plant_mod_blocking(&game, "Alpha", "alpha", &[("BETA", "(2b) Beta Title")]);
    plant_mod_blocking(&game, "Beta", "beta", &[]);

    assert_eq!(offer_for(&game, &neither(&[])).0, ["Alpha", "Beta"]);

    let (offered, refused) = offer_for(&game, &neither(&["Alpha"]));
    assert_eq!(offered, ["Alpha"]);
    assert_eq!(refused, ["Beta: it can't run alongside Alpha"]);

    let (offered, refused) = offer_for(&game, &neither(&["Beta"]));
    assert_eq!(offered, ["Beta"]);
    assert_eq!(refused, ["Alpha: it can't run alongside Beta Title"]);
}

/// Managed mods always win, whichever side declares it, and only while they are deployed.
#[test]
fn a_block_against_a_managed_mod_goes_against_the_players_mod() {
    let game = modpack_game();
    plant_mod_blocking(
        &game,
        "Blocks VP",
        "blocks-vp",
        &[(VOX_POPULI_ID, "(2) Vox Populi")],
    );
    plant_mod_blocking(
        &game,
        "More Luxuries",
        "BE2D13EE-D18D-410E-8F04-7CD19CD99A98",
        &[],
    );
    plant_mod_blocking(
        &game,
        "TechTree",
        "e5cc542a-1580-4ca1-b44f-c29c985cf990",
        &[],
    );

    let (offered, refused) = offer_for(&game, &with_flavor(Flavor::CommunityPatch, &[]));
    assert_eq!(offered, ["Blocks VP", "More Luxuries", "TechTree"]);
    assert_eq!(refused, Vec::<String>::new());

    let (offered, refused) = offer_for(&game, &modpack_with(&["Blocks VP", "More Luxuries"]));
    assert_eq!(offered, ["TechTree"]);
    assert_eq!(
        refused,
        [
            "Blocks VP: it can't run alongside Vox Populi",
            "More Luxuries: it can't run alongside Vox Populi",
        ]
    );

    // `(3a)` blocks this one, so it goes with EUI.
    let (offered, refused) = offer_for(
        &game,
        &with_flavor(Flavor::VoxPopuli { eui: Eui::Enabled }, &[]),
    );
    assert_eq!(offered, Vec::<String>::new());
    assert_eq!(
        refused[2],
        "TechTree: it can't run alongside VP - EUI Compatibility Files"
    );
}

/// Ticks that clash anyway, loaded from settings say: the first in the selection's order stays
/// and the other goes, whichever of them declares it.
#[test]
fn of_two_ticked_mods_that_clash_the_first_is_kept() {
    let game = modpack_game();
    plant_mod_blocking(&game, "Alpha", "alpha", &[("beta", "Beta")]);
    plant_mod_blocking(&game, "Beta", "beta", &[]);

    let (offered, refused) = offer_for(&game, &neither(&["Alpha", "Beta"]));
    assert_eq!(offered, ["Alpha"]);
    assert_eq!(refused, ["Beta: it can't run alongside Alpha"]);

    let (offered, refused) = offer_for(&game, &neither(&["Beta", "Alpha"]));
    assert_eq!(offered, ["Beta"]);
    assert_eq!(refused, ["Alpha: it can't run alongside Beta"]);
}

/// Dropping a mod can resolve a clash, so they go one at a time: in a chain the middle one
/// goes and both ends stay. Mods that block each other in a ring still settle, with one left.
#[test]
fn clashes_settle_one_at_a_time_and_always_finish() {
    let game = modpack_game();
    plant_mod_blocking(&game, "A", "a", &[("b", "B")]);
    plant_mod_blocking(&game, "B", "b", &[("c", "C")]);
    plant_mod_blocking(&game, "C", "c", &[]);
    assert_eq!(
        offer_for(&game, &neither(&["A", "B", "C"])).0,
        ["A", "C"],
        "B went, and nothing is left to hold against C"
    );

    let ring = modpack_game();
    plant_mod_blocking(&ring, "A", "a", &[("b", "B")]);
    plant_mod_blocking(&ring, "B", "b", &[("a", "A"), ("c", "C")]);
    plant_mod_blocking(&ring, "C", "c", &[("a", "A")]);
    let (offered, refused) = offer_for(&ring, &neither(&["A", "B", "C"]));
    assert_eq!(offered, ["A"]);
    assert_eq!(refused.len(), 2, "{refused:?}");
}

/// A mod that will not make it anyway holds nothing against a neighbour, and a mod that loses a
/// clash stops counting for whatever depended on it.
#[test]
fn clashes_and_dependencies_settle_together() {
    let game = modpack_game();
    game.plant(
        "MODS/A Needs VP/A Needs VP.modinfo",
        "<Mod id=\"a-needs-vp\" version=\"1\"><Dependencies>\
         <Mod id=\"8411a7a8-dad3-4622-a18e-fcc18324c799\" title=\"(2) Vox Populi\" />\
         </Dependencies><Blocks><Mod id=\"beta\" title=\"Beta\" /></Blocks></Mod>",
    );
    plant_mod_blocking(&game, "Beta", "beta", &[("delta", "Delta")]);
    plant_mod_blocking(&game, "Delta", "delta", &[]);
    plant_mod_needing(&game, "Needs Delta", "needs-delta", &[("delta", "Delta")]);

    // "A Needs VP" falls for want of Vox Populi, so Beta stays, and Delta loses to Beta.
    let (offered, refused) = offer_for(
        &game,
        &neither(&["A Needs VP", "Beta", "Delta", "Needs Delta"]),
    );
    assert_eq!(offered, ["Beta"]);
    assert_eq!(
        refused,
        [
            "A Needs VP: it needs Vox Populi",
            "Delta: it can't run alongside Beta",
            "Needs Delta: it needs Delta",
        ]
    );
}

/// Only a mod can block a mod: `<Dlc>` and `<Game>` lines are about the game, and a mod listing
/// its own ID does not stand in its own way.
#[test]
fn only_a_block_on_another_mod_counts() {
    let game = modpack_game();
    game.plant(
        "MODS/Odd/Odd.modinfo",
        "<Mod id=\"odd\" version=\"1\"><Blocks>\
         <Dlc id=\"beta\" minversion=\"0\" maxversion=\"999\" />\
         <Game id=\"beta\" />\
         <Mod id=\"ODD\" title=\"Odd\" />\
         </Blocks></Mod>",
    );
    plant_mod_blocking(&game, "Beta", "beta", &[]);

    let (offered, refused) = offer_for(&game, &neither(&["Beta", "Odd"]));
    assert_eq!(offered, ["Beta", "Odd"]);
    assert_eq!(refused, Vec::<String>::new());
}

/// The build goes by the same rule and stops with the reason rather than baking both.
#[test]
fn the_build_will_not_bake_two_mods_that_clash() {
    let game = modpack_game();
    plant_mod_blocking(&game, "Alpha", "alpha", &[("beta", "Beta")]);
    plant_mod_blocking(&game, "Beta", "beta", &[]);
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let before = game.files();
    let plan = core
        .plan(&neither(&["Alpha", "Beta"]), &game.folders())
        .unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a pack is no place for two mods the game would not run together");
    assert!(
        error
            .user_message()
            .contains("\"Beta\" cannot be baked into the Modpack: it can't run alongside Alpha"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
}

/// The offer knows the managed mods' blocks from a table, because it is shown before the sources
/// are in hand. The build reads the modinfos staged, so a block newer than the table still
/// stops it, and it says who.
#[test]
fn the_build_reads_the_blocks_of_the_managed_mods_really_staged() {
    let game = modpack_game();
    plant_mod_blocking(
        &game,
        "Newer Clash",
        "f1x7ure0-only-in-the-staged-modinfo0000",
        &[],
    );
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let (offered, _) = offer_for(&game, &modpack_with(&["Newer Clash"]));
    assert_eq!(offered, ["Newer Clash"]);

    let before = game.files();
    let plan = core
        .plan(&modpack_with(&["Newer Clash"]), &game.folders())
        .unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("the staged Vox Populi blocks this mod");
    assert!(
        error
            .user_message()
            .contains("it can't run alongside Vox Populi"),
        "got: {}",
        error.user_message()
    );
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
}

/// Without the Community Patch the Vox Populi intro has no place: a saved configuration that
/// still asks for it deploys nothing, and an intro swapped in earlier goes the way it goes when
/// the box is cleared - the choice itself survives for when a Flavor with it is picked again.
#[test]
fn neither_treats_the_vox_populi_intro_as_cleared() {
    let game = modpack_game();
    plant_players_own_mods(&game);
    let movie = "Assets/DLC/Expansion2/Civ5XP2_Opening_Movie_en_US.wmv";
    let original = "Brave New World's own opening movie";
    game.plant(movie, original);
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let neither = InstallConfiguration {
        opening_movie: civ5vp_core::OpeningMovie::VoxPopuli,
        ..own_mods_only_modpack()
    };

    // Never swapped: left alone.
    let plan = core.plan(&neither, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert_eq!(game.read(movie), original);

    // Swapped by an earlier install: put back.
    let swapped = InstallConfiguration {
        opening_movie: civ5vp_core::OpeningMovie::VoxPopuli,
        ..vox_populi_modpack()
    };
    let plan = core.plan(&swapped, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert!(std::fs::read(game.game_root().join(movie)).unwrap() != original.as_bytes());

    let plan = core.plan(&neither, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert_eq!(game.read(movie), original);
}

/// A small mod in MODS carrying these files.
fn plant_mod_with_files(game: &GameFixture, folder: &str, files: &[&str]) {
    game.plant(
        &format!("MODS/{folder}/{folder}.modinfo"),
        &format!("<Mod id=\"{folder}\" version=\"1\"/>"),
    );
    for file in files {
        game.plant(&format!("MODS/{folder}/{file}"), "-- fixture");
    }
}

/// The notes of the offer, as `mod: note`.
fn notes_for(game: &GameFixture, configuration: &InstallConfiguration) -> Vec<String> {
    civ5vp_core::extra_mod_offer(&game.folders().mods, configuration)
        .notes
        .iter()
        .map(|note| format!("{}: {}", note.name, note.note))
        .collect()
}

/// Mods often replace files on purpose, so this is a note and nothing more: both stay
/// tickable, and each says who the other is and whose copy wins - the later in the selection,
/// matched by bare name at any depth, ignoring case.
#[test]
fn two_ticked_mods_with_a_same_named_file_get_a_note_and_nothing_else() {
    let game = modpack_game();
    plant_mod_with_files(&game, "Alpha", &["Shared.lua"]);
    plant_mod_with_files(&game, "Beta", &["Deep/Inside/SHARED.LUA"]);

    assert_eq!(notes_for(&game, &neither(&["Alpha"])), Vec::<String>::new());

    let both = neither(&["Alpha", "Beta"]);
    assert_eq!(
        notes_for(&game, &both),
        [
            "Alpha: shares SHARED.LUA with Beta: Beta's copy wins",
            "Beta: shares SHARED.LUA with Alpha: Beta's copy wins",
        ]
    );
    let (offered, refused) = offer_for(&game, &both);
    assert_eq!(offered, ["Alpha", "Beta"]);
    assert_eq!(refused, Vec::<String>::new());

    assert_eq!(
        notes_for(&game, &neither(&["Beta", "Alpha"])),
        [
            "Beta: shares Shared.lua with Alpha: Alpha's copy wins",
            "Alpha: shares Shared.lua with Beta: Alpha's copy wins",
        ],
        "the selection's order decides"
    );
}

/// `.lua`, `.xml` and `.sql` only. The UI entry files are left out: staging replaces those by
/// design and hooks whichever copy ends up in the pack. A file repeated inside one mod is that
/// mod's own affair.
#[test]
fn only_files_that_can_shadow_each_other_get_a_note() {
    let game = modpack_game();
    plant_mod_with_files(
        &game,
        "Alpha",
        &[
            "Art.dds",
            "InGame.lua",
            "MapGenerator.lua",
            "Notes.txt",
            "Twice.lua",
            "Sub/Twice.lua",
        ],
    );
    plant_mod_with_files(
        &game,
        "Beta",
        &["Art.dds", "Sub/INGAME.lua", "MapGenerator.lua", "Notes.txt"],
    );

    assert_eq!(
        notes_for(&game, &neither(&["Alpha", "Beta"])),
        Vec::<String>::new()
    );
}

/// A mod overriding a lot of files gets one line, not one per file.
#[test]
fn a_long_list_of_shared_files_is_cut_short() {
    let game = modpack_game();
    let files = ["A.lua", "B.xml", "C.sql", "D.lua", "E.lua"];
    plant_mod_with_files(&game, "Alpha", &files);
    plant_mod_with_files(&game, "Beta", &files);

    assert_eq!(
        notes_for(&game, &neither(&["Alpha", "Beta"]))[0],
        "Alpha: shares A.lua, B.xml, C.sql and 2 more with Beta: Beta's copies win"
    );
}

/// A mod that was refused is not going in, so it shadows nothing.
#[test]
fn a_refused_mod_gets_and_causes_no_note() {
    let game = modpack_game();
    plant_mod_with_files(&game, "Alpha", &["Shared.lua"]);
    plant_mod_with_files(&game, "Beta", &["Shared.lua"]);
    plant_mod_blocking(&game, "Beta", "Beta", &[("Alpha", "Alpha")]);

    assert_eq!(
        notes_for(&game, &neither(&["Alpha", "Beta"])),
        Vec::<String>::new()
    );
}

/// The managed mods count when their files are on disk, by what the configuration really
/// deploys: with EUI, `(2)`'s `LUA` folder stays behind, and a mod's copy shadows nothing there.
/// A player's mod comes after them, so its copy wins. With nothing fetched there is nothing to
/// say, and nothing is said.
#[test]
fn a_file_shared_with_a_managed_mod_gets_a_note_when_the_source_is_on_disk() {
    let game = modpack_game();
    plant_mod_with_files(&game, "Alpha", &["PlotHelpManager.lua"]);
    let core = core_over(&game, FixtureModpackAssembler::ignored());
    let vox_populi = modpack_with(&["Alpha"]);

    let scan = core.extra_mod_scan(&game.folders().mods, &vox_populi.source);
    let notes = scan.offer(&vox_populi).notes;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].name, "Alpha");
    assert_eq!(
        notes[0].note,
        "shares PlotHelpManager.lua with Vox Populi: Alpha's copy wins"
    );

    let with_eui = with_flavor(Flavor::VoxPopuli { eui: Eui::Enabled }, &["Alpha"]);
    assert_eq!(scan.offer(&with_eui).notes, Vec::new());

    let not_fetched = civ5vp_core::ExtraModScan::of(&game.folders().mods);
    assert_eq!(not_fetched.offer(&vox_populi).notes, Vec::new());
}

/// The build goes ahead, bakes both, and leaves the note in the log - against the managed mods
/// too, whose files are in hand by then.
#[test]
fn the_build_proceeds_and_logs_same_named_files() {
    let game = modpack_game();
    plant_mod_with_files(&game, "Alpha", &["PlotHelpManager.lua", "Shared.lua"]);
    plant_mod_with_files(&game, "Beta", &["Shared.lua"]);
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let plan = core
        .plan(&modpack_with(&["Alpha", "Beta"]), &game.folders())
        .unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .unwrap();

    let heads_ups: Vec<String> = receiver
        .try_iter()
        .filter(|event| event.heads_up)
        .map(|event| event.message)
        .collect();
    assert_eq!(
        heads_ups,
        [
            "Vox Populi and Alpha both have PlotHelpManager.lua: Alpha's copy wins.",
            "Alpha and Beta both have Shared.lua: Beta's copy wins.",
        ]
    );
    for baked in ["Alpha/Shared.lua", "Beta/Shared.lua"] {
        assert!(
            game.game_root()
                .join("DLC/VP_MODPACK/Mods")
                .join(baked)
                .exists()
        );
    }
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
        civ5vp_core::extra_mod_offer(&game.folders().mods, &vox_populi_modpack())
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

/// A Modpack of the player's own mods alone: neither Community Patch nor Vox Populi.
fn own_mods_only_modpack() -> InstallConfiguration {
    InstallConfiguration {
        // Nothing comes from an Installation Source, so none has to be named.
        source: InstallationSource::unchosen(),
        flavor: Flavor::OwnModsOnly,
        extra_mods: vec!["Even More Bonuses".to_owned()],
        ..vox_populi_modpack()
    }
}

/// The whole of "a Modpack without Community Patch or Vox Populi": the pick is baked into
/// the bare game, and nothing is fetched or built for it - the source provider and the
/// toolchain here fail on contact, so a single call to either fails the test.
#[test]
fn a_modpack_of_only_the_players_own_mods_takes_nothing_from_the_installation_source() {
    let game = modpack_game();
    plant_players_own_mods(&game);
    // A Vox Populi Modpack from an earlier day, the VP-only DLC beside it, VP's tips in the
    // Text Folder - and a Mods-mode install, which a Modpack Deployment leaves alone.
    game.plant(
        "DLC/VP_MODPACK/Mods/(1) Community Patch/CvGameCore_Expansion2.dll",
        "an older Modpack's DLL",
    );
    game.plant("DLC/VPUI/VPUI.Civ5Pkg", "Vox Populi's interface");
    game.plant("Text/VPUI_tips_en_us.xml", "Vox Populi's loading tips");
    game.plant("MODS/(1) Community Patch/existing.txt", "left alone");
    // That Modpack left the snapshot it was built from.
    let snapshot = game.work_dir().join("modpack-base");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::write(snapshot.join("Civ5DebugDatabase.db"), "pristine base").unwrap();
    std::fs::write(snapshot.join("Localization-Merged.db"), "pristine text").unwrap();
    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = Core::new(
        Box::new(support::FailingSourceProvider),
        Box::new(support::FailingToolchainRunner),
        Box::new(assembler),
        game.work_dir(),
    );

    let configuration = InstallConfiguration {
        // 43 Civs changes which DLL goes into `(1)`; with no `(1)` it has nothing to apply to.
        forty_three_civs: FortyThreeCivs::Enabled,
        ..own_mods_only_modpack()
    };
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    let outcome = core
        .execute(&plan, &ProgressReporter::silent())
        .expect("a pack of the player's own mods needs no download and no build");

    let pack = game.folders().dlc.join("VP_MODPACK");
    assert!(pack.join("MPModsPack.Civ5Pkg").is_file());
    let mut packed: Vec<String> = std::fs::read_dir(pack.join("Mods"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    packed.sort_unstable();
    assert_eq!(packed, ["Even More Bonuses"], "the pick, and only the pick");
    assert_eq!(outcome.built_dll, None, "no Community Patch, no DLL");

    // Only the pick's changes cross the seam - no Vox Populi text to put under them.
    let jobs = jobs.lock().unwrap();
    let updates: Vec<String> = jobs[0]
        .updates
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(updates, ["MoreBonuses.sql"]);

    // Vox Populi's DLC and tips do not belong to this configuration.
    assert_eq!(outcome.deployed, [ClaimedFolder::Modpack]);
    assert!(!game.folders().dlc.join("VPUI").exists());
    assert!(!game.folders().text.join("VPUI_tips_en_us.xml").exists());
    // MODS is only read, as for every Modpack.
    assert_eq!(
        game.read("MODS/(1) Community Patch/existing.txt"),
        "left alone"
    );
    assert!(game.folders().mods.join("Even More Bonuses").is_dir());
    // Nothing installed carries a Version, so no banner can claim one.
    assert_eq!(
        core.installed_version_label(&game.folders(), InstallMode::Modpack),
        None
    );
}

/// In Mods mode the player's own mods are in MODS already, so without Community Patch or Vox
/// Populi there is nothing to install. Refused before anything happens, with a way out.
#[test]
fn own_mods_only_is_refused_as_a_mods_deployment() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let Err(error) = core.plan(&mods_mode(own_mods_only_modpack()), &game.folders()) else {
        panic!("a Mods Deployment of nothing must be refused");
    };

    assert!(matches!(
        error,
        InstallError::UnsupportedConfiguration { .. }
    ));
    assert!(
        error.user_message().contains("Install as a modpack"),
        "got: {}",
        error.user_message()
    );
}

/// A Modpack of the player's own mods with none of them picked would be the bare game baked
/// into a DLC - certainly not what anyone asked for.
#[test]
fn an_own_mods_only_modpack_needs_at_least_one_mod() {
    let game = modpack_game();
    let core = core_over(&game, FixtureModpackAssembler::ignored());

    let configuration = InstallConfiguration {
        extra_mods: Vec::new(),
        ..own_mods_only_modpack()
    };
    let Err(error) = core.plan(&configuration, &game.folders()) else {
        panic!("an empty Modpack must be refused");
    };

    assert!(matches!(
        error,
        InstallError::UnsupportedConfiguration { .. }
    ));
    assert!(
        error.user_message().contains("at least one"),
        "got: {}",
        error.user_message()
    );
}

/// What the shell asks before drawing the choices: which Flavors bring the Community Patch
/// (and so a DLL, and 43 Civs), and which make sense in which Install Mode.
#[test]
fn only_a_modpack_can_hold_neither_community_patch_nor_vox_populi() {
    for flavor in [
        Flavor::CommunityPatch,
        Flavor::VoxPopuli { eui: Eui::Disabled },
        Flavor::VoxPopuli { eui: Eui::Enabled },
    ] {
        assert!(flavor.includes_community_patch(), "{flavor:?}");
        assert!(flavor.is_legal_with(InstallMode::Mods), "{flavor:?}");
        assert!(flavor.is_legal_with(InstallMode::Modpack), "{flavor:?}");
    }
    assert!(!Flavor::OwnModsOnly.includes_community_patch());
    assert!(!Flavor::OwnModsOnly.is_legal_with(InstallMode::Mods));
    assert!(Flavor::OwnModsOnly.is_legal_with(InstallMode::Modpack));
}

/// Nothing is compiled for a pack of the player's own mods - not even where every other
/// Flavor would compile - unless the player also asked for LuaJIT, which is always built.
#[test]
fn a_modpack_of_only_own_mods_needs_the_toolchain_only_for_luajit() {
    let mut configuration = InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: miniature_repo(),
        },
        dll_source: DllSource::AlwaysCompile,
        ..own_mods_only_modpack()
    };
    assert!(!configuration.needs_the_toolchain());

    configuration.luajit = LuaJitEngine::LuaJit;
    assert!(configuration.needs_the_toolchain());
}
