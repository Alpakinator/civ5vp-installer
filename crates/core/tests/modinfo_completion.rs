//! Modinfo Completion (ADR-0014): a managed mod's `.modinfo`, as deployed, gains what its
//! `.civ5proj` lists and the committed file lacks - the way ModBuddy's Build would have written
//! it - and nothing else about the file changes.
//!
//! The oracle is upstream's own `scripts/generate_modinfo.py`: `fixtures/completion` holds a
//! small mod with its `.civ5proj`, and beside it the modinfo that script generates for it
//! (`… .modinfo.generated`, made once with the POSIX wrapper in
//! `.scratch/civ5vp-installer/tools/modinfo/gen_posix.py`). A modinfo missing some of that
//! output must deploy as exactly that output.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{
    BuildConfiguration, Core, DllSource, Eui, Flavor, FortyThreeCivs, InstallConfiguration,
    InstallMode, InstallationSource, LuaJitEngine, ProgressReporter, Version,
};
use support::{
    FixtureModpackAssembler, FixtureSourceProvider, GameFixture, MarkerToolchainRunner,
    miniature_repo,
};

const SQUADS: &str = "(4a) Squads for VP";
const SQUADS_MODINFO: &str = "(4a) Squads for VP (v 1).modinfo";

fn completion_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/completion")
}

/// What `generate_modinfo.py` writes for the fixture Squads: BOM, `\n` line ends.
fn generated_squads_modinfo() -> String {
    fs::read_to_string(completion_fixtures().join(format!("{SQUADS_MODINFO}.generated"))).unwrap()
}

/// A private, editable copy of the miniature repository whose `(4a)` is the fixture Squads,
/// committed with `modinfo` - so a Local Repo, used exactly as it is.
fn checkout_with_squads(game: &GameFixture, modinfo: &str) -> PathBuf {
    let checkout = game.work_dir().join("checkout");
    copy_tree(&miniature_repo(), &checkout);
    fs::remove_dir_all(checkout.join(SQUADS)).unwrap();
    copy_tree(&completion_fixtures().join(SQUADS), &checkout.join(SQUADS));
    fs::write(checkout.join(SQUADS).join(SQUADS_MODINFO), modinfo).unwrap();
    checkout
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap().path();
        let target = to.join(entry.file_name().unwrap());
        if entry.is_dir() {
            copy_tree(&entry, &target);
        } else {
            fs::copy(&entry, &target).unwrap();
        }
    }
}

fn configuration(checkout: &Path) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: checkout.to_path_buf(),
        },
        flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
        menu_theme: civ5vp_core::MenuTheme::Stock,
        opening_movie: civ5vp_core::OpeningMovie::Stock,
        dll_source: DllSource::ShippedWhenCurrent,
    }
}

fn core_over(game: &GameFixture, checkout: &Path, assembler: FixtureModpackAssembler) -> Core {
    Core::new(
        Box::new(FixtureSourceProvider::new(checkout.to_path_buf())),
        Box::new(MarkerToolchainRunner),
        Box::new(assembler),
        game.work_dir(),
    )
}

/// Deploy `configuration` and return the run's heads-up lines - the ones kept in the log file.
fn deploy(core: &Core, game: &GameFixture, configuration: &InstallConfiguration) -> Vec<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core.plan(configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .unwrap();
    receiver
        .try_iter()
        .filter(|event| event.heads_up)
        .map(|event| event.message)
        .collect()
}

/// `text` without the lines `drop` matches, each with the `follow` lines after it.
fn without_lines(text: &str, drop: &[(&str, usize)]) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut kept = Vec::new();
    let mut skip = 0;
    for line in lines {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if let Some((_, follow)) = drop.iter().find(|(needle, _)| line.contains(needle)) {
            skip = *follow;
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

/// The round trip: the generated modinfo with one action, one `<File>` and one entry point taken
/// out deploys as the generated modinfo again, byte for byte - each line back in its place, md5
/// and all. With `\r\n` line ends too, which the added lines follow; and twice, the same.
#[test]
fn a_modinfo_missing_lines_deploys_as_modbuddy_would_have_written_it() {
    for newline in ["\n", "\r\n"] {
        let generated = generated_squads_modinfo().replace('\n', newline);
        let committed = without_lines(
            &generated,
            &[
                // An action with a neighbour before it in its set.
                ("<UpdateDatabase>DB\\SquadsLate.sql</UpdateDatabase>", 0),
                // A file between two the modinfo keeps.
                (">UI/SquadNames.xml</File>", 0),
                // The first entry point: nothing before it, so it goes first in its section.
                ("file=\"UI/Squads.xml\"", 3),
            ],
        );
        assert_ne!(committed, generated, "the fixture really lacks something");
        let game = GameFixture::new();
        let checkout = checkout_with_squads(&game, &committed);
        let core = core_over(&game, &checkout, FixtureModpackAssembler::ignored());

        let kept = deploy(&core, &game, &configuration(&checkout));

        let deployed = fs::read(
            game.game_root()
                .join("MODS")
                .join(SQUADS)
                .join(SQUADS_MODINFO),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&deployed),
            generated,
            "deployed as generate_modinfo.py writes it ({newline:?} line ends)"
        );
        assert_eq!(
            kept,
            [format!(
                "Completed {SQUADS}'s .modinfo from its .civ5proj, as ModBuddy would have: added \
                 the file UI/SquadNames.xml, the action DB/SquadsLate.sql, the entry point \
                 UI/Squads.xml."
            )],
            "one line naming what was added, kept in the log file"
        );
        // Rule 6: a Local Repo is the developer's checkout, and stays exactly as it was.
        assert_eq!(
            fs::read_to_string(checkout.join(SQUADS).join(SQUADS_MODINFO)).unwrap(),
            committed,
            "the checkout's own modinfo is untouched"
        );

        // Rule 8: the same inputs deploy the same bytes.
        deploy(&core, &game, &configuration(&checkout));
        let again = fs::read(
            game.game_root()
                .join("MODS")
                .join(SQUADS)
                .join(SQUADS_MODINFO),
        )
        .unwrap();
        assert_eq!(again, deployed);
    }
}

/// A modinfo that lacks nothing its `.civ5proj` lists is the committed file, byte for byte, with
/// nothing said - which is why a Release installs exactly as the official installer installs it.
#[test]
fn a_complete_modinfo_deploys_byte_for_byte_and_says_nothing() {
    let game = GameFixture::new();
    let generated = generated_squads_modinfo();
    let checkout = checkout_with_squads(&game, &generated);
    let core = core_over(&game, &checkout, FixtureModpackAssembler::ignored());

    let kept = deploy(&core, &game, &configuration(&checkout));

    assert_eq!(
        game.read(&format!("MODS/{SQUADS}/{SQUADS_MODINFO}")),
        generated
    );
    assert_eq!(kept, Vec::<String>::new());
}

/// Only what this configuration deploys is added - never a line pointing at nothing. EUI strips
/// `LUA/` from `(2)`, so a Lua file the `.civ5proj` lists there is left out with EUI and added
/// without it; a database file outside `LUA/` is added either way.
#[test]
fn with_eui_a_file_under_lua_is_not_added() {
    for (eui, lua_added) in [(Eui::Enabled, false), (Eui::Disabled, true)] {
        let game = GameFixture::new();
        let checkout = checkout_with_squads(&game, &generated_squads_modinfo());
        let vox_populi = checkout.join("(2) Vox Populi");
        fs::write(vox_populi.join("LUA/NewPanel.lua"), "-- a new panel\n").unwrap();
        fs::write(
            vox_populi.join("Balance Changes/Extra.sql"),
            "UPDATE Defines SET Value = 3;\n",
        )
        .unwrap();
        fs::write(
            vox_populi.join("Vox Populi.civ5proj"),
            project(
                &[
                    "Balance Changes\\BalanceChanges.sql",
                    "Balance Changes\\Extra.sql",
                ],
                &[
                    ("Balance Changes\\BalanceChanges.sql", false),
                    ("Balance Changes\\Extra.sql", false),
                    ("LUA\\NewPanel.lua", true),
                    ("LUA\\PlotHelpManager.lua", true),
                ],
            ),
        )
        .unwrap();
        let core = core_over(&game, &checkout, FixtureModpackAssembler::ignored());
        let mut configuration = configuration(&checkout);
        configuration.flavor = Flavor::VoxPopuli { eui };

        deploy(&core, &game, &configuration);

        let deployed = game.read("MODS/(2) Vox Populi/(2) Vox Populi.modinfo");
        assert!(
            deployed
                .contains("      <UpdateDatabase>Balance Changes\\Extra.sql</UpdateDatabase>\n"),
            "{deployed}"
        );
        assert!(
            deployed.contains(">Balance Changes/Extra.sql</File>"),
            "{deployed}"
        );
        assert_eq!(
            deployed.contains("LUA/NewPanel.lua"),
            lua_added,
            "EUI {eui:?}: {deployed}"
        );
    }
}

/// With 43 Civs, `(3b)` deploys only its modinfo and `AdvancedSetup.lua`: the `.civ5proj`'s other
/// files are not added - not its readme, not the DLL, which goes into `(1)` - and the one it does
/// deploy is, in a `<Files>` section the committed modinfo did not have.
#[test]
fn with_43_civs_nothing_is_added_to_3b_for_files_it_does_not_deploy() {
    let game = GameFixture::new();
    let checkout = checkout_with_squads(&game, &generated_squads_modinfo());
    let forty_three = checkout.join("(3b) 43 Civs Community Patch");
    fs::write(
        forty_three.join("43 Civs Community Patch.civ5proj"),
        project(
            &[],
            &[
                ("AdvancedSetup.lua", true),
                ("CvGameCore_Expansion2.dll", true),
                ("Kit\\ReadMe.txt", false),
            ],
        ),
    )
    .unwrap();
    let core = core_over(&game, &checkout, FixtureModpackAssembler::ignored());
    let mut configuration = configuration(&checkout);
    configuration.forty_three_civs = FortyThreeCivs::Enabled;

    deploy(&core, &game, &configuration);

    assert_eq!(
        game.read("MODS/(3b) 43 Civs Community Patch/(3b) 43 Civs Community Patch (v 1).modinfo"),
        "<Mod id=\"6521c02b-1c78-4826-a5b0-faed86235e98\" version=\"1\">\n\
         \x20 <Properties>\n\
         \x20   <Name>(3b) 43 Civs Community Patch</Name>\n\
         \x20 </Properties>\n\
         \x20 <Files>\n\
         \x20   <File md5=\"AC34333A6DAE2C880A6390F92351C9CD\" import=\"1\">AdvancedSetup.lua</File>\n\
         \x20 </Files>\n\
         </Mod>\n"
    );
}

/// The Modpack merges the database updates of the modinfo the game would read: an action the
/// committed modinfo lacks is in the update list at its `.civ5proj` position. From an Upstream
/// Version too - completion is not only for a developer's checkout.
#[test]
fn the_modpack_merges_a_missing_action_at_its_civ5proj_position() {
    let game = GameFixture::new();
    for (file, contents) in [
        (
            "DLC/Expansion2/UI/InGame/InGame.lua",
            "-- base InGame.lua\n",
        ),
        (
            "DLC/Expansion2/UI/InGame/CityView/CityView.lua",
            "-- base CityView.lua\n",
        ),
        (
            "DLC/Expansion2/UI/InGame/LeaderHead/LeaderHeadRoot.lua",
            "-- base LeaderHeadRoot.lua\n",
        ),
        ("cache/Civ5DebugDatabase.db", "pristine base"),
        ("cache/Localization-Merged.db", "pristine text"),
    ] {
        game.plant(file, contents);
    }
    let checkout = checkout_with_squads(&game, &generated_squads_modinfo());
    let vox_populi = checkout.join("(2) Vox Populi");
    for name in ["Victory.sql", "Late.sql"] {
        fs::write(vox_populi.join("Balance Changes").join(name), "-- sql\n").unwrap();
    }
    // Committed with the first and the last, as upstream's (1) was at 62c77de6f.
    let modinfo = vox_populi.join("(2) Vox Populi.modinfo");
    let committed = fs::read_to_string(&modinfo).unwrap().replace(
        "      <UpdateDatabase>Balance Changes/BalanceChanges.sql</UpdateDatabase>\n",
        "      <UpdateDatabase>Balance Changes/BalanceChanges.sql</UpdateDatabase>\n\
         \x20     <UpdateDatabase>Balance Changes/Late.sql</UpdateDatabase>\n",
    );
    fs::write(&modinfo, committed).unwrap();
    fs::write(
        vox_populi.join("Vox Populi.civ5proj"),
        project(
            &[
                "Balance Changes\\BalanceChanges.sql",
                "Balance Changes\\Victory.sql",
                "Balance Changes\\Late.sql",
            ],
            &[],
        ),
    )
    .unwrap();
    let (assembler, jobs) = FixtureModpackAssembler::new();
    let core = core_over(&game, &checkout, assembler);
    let mut configuration = configuration(&checkout);
    configuration.install_mode = InstallMode::Modpack;
    configuration.source = InstallationSource::UpstreamCache {
        version: Version::Release("Release-2.0".to_owned()),
    };

    deploy(&core, &game, &configuration);

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
            "Victory.sql",
            "Late.sql",
            "Squads_EN_US.xml",
            "Squads.sql",
            "SquadsLate.sql",
        ]
    );
}

/// A `.civ5proj` that cannot be read is no reason to fail an install: the committed modinfo
/// deploys, as it did before completion existed, and the log file says why.
#[test]
fn a_broken_civ5proj_is_logged_and_the_committed_modinfo_deploys() {
    let game = GameFixture::new();
    let committed = without_lines(&generated_squads_modinfo(), &[(">UI/Squads.lua</File>", 0)]);
    let checkout = checkout_with_squads(&game, &committed);
    fs::write(
        checkout.join(SQUADS).join("Squads.civ5proj"),
        "<Project>half a project",
    )
    .unwrap();
    let core = core_over(&game, &checkout, FixtureModpackAssembler::ignored());

    let kept = deploy(&core, &game, &configuration(&checkout));

    assert_eq!(
        game.read(&format!("MODS/{SQUADS}/{SQUADS_MODINFO}")),
        committed
    );
    assert_eq!(
        kept,
        [format!(
            "Could not read {SQUADS}'s Squads.civ5proj (it has no <Project> element), so its \
             .modinfo is installed as it is."
        )]
    );
}

/// Dev mode holds a checkout against the modinfo the game will read: a file the `.civ5proj`
/// lists and completion adds is not reported as unlisted.
#[test]
fn dev_mode_checks_the_files_against_the_completed_modinfo() {
    let game = GameFixture::new();
    let committed = without_lines(
        &generated_squads_modinfo(),
        &[(">UI/SquadNames.lua</File>", 0)],
    );
    let checkout = checkout_with_squads(&game, &committed);
    let core = core_over(&game, &checkout, FixtureModpackAssembler::ignored());

    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core
        .plan(&configuration(&checkout), &game.folders())
        .unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .unwrap();

    let unlisted: Vec<String> = receiver
        .try_iter()
        .map(|event| event.message)
        .filter(|message| message.contains("does not list"))
        .collect();
    assert_eq!(unlisted, Vec::<String>::new());
}

/// A minimal ModBuddy project: `UpdateDatabase` actions on activation, and the files with their
/// import flag - the shape upstream's projects have.
fn project(actions: &[&str], files: &[(&str, bool)]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <Project DefaultTargets=\"Deploy\" ToolsVersion=\"4.0\" \
         xmlns=\"http://schemas.microsoft.com/developer/msbuild/2003\">\n\
         \x20 <PropertyGroup>\n\
         \x20   <ModActions>\n",
    );
    for action in actions {
        xml.push_str(&format!(
            "      <Action>\n        <Set>OnModActivated</Set>\n        \
             <Type>UpdateDatabase</Type>\n        <FileName>{action}</FileName>\n      \
             </Action>\n"
        ));
    }
    xml.push_str("    </ModActions>\n  </PropertyGroup>\n  <ItemGroup>\n");
    for (file, import) in files {
        xml.push_str(&format!(
            "    <Content Include=\"{file}\">\n      <SubType>Lua</SubType>\n      \
             <ImportIntoVFS>{}</ImportIntoVFS>\n    </Content>\n",
            if *import { "True" } else { "False" }
        ));
    }
    xml.push_str("  </ItemGroup>\n</Project>\n");
    xml
}
