//! Exercise the real database assembler through the Core, with fixture sources and a
//! marker compiler. In particular, a distributed Modpack must carry the text that the
//! creator would otherwise see only through their Documents/Text folder (#13364).

#[path = "../../core/tests/support/mod.rs"]
mod support;

use civ5vp_core::{
    BuildConfiguration, Core, DllSource, Eui, Flavor, FortyThreeCivs, InstallConfiguration,
    InstallMode, InstallationSource, LuaJitEngine, MenuTheme, ProgressReporter,
};
use civ5vp_modpack::SqliteModpackAssembler;
use rusqlite::Connection;
use support::{FixtureSourceProvider, GameFixture, MarkerToolchainRunner};

fn miniature_repo() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/miniature-repo")
}

#[test]
fn a_distributed_modpack_carries_vpui_text_from_the_selected_source() {
    for flavor in [
        Flavor::VoxPopuli { eui: Eui::Enabled },
        Flavor::VoxPopuli { eui: Eui::Disabled },
        Flavor::CommunityPatch,
    ] {
        let game = GameFixture::new();
        for file in [
            "DLC/Expansion2/UI/InGame/InGame.lua",
            "DLC/Expansion2/UI/InGame/CityView/CityView.lua",
            "DLC/Expansion2/UI/InGame/LeaderHead/LeaderHeadRoot.lua",
        ] {
            game.plant(file, "-- fixture base UI");
        }
        let base = game.work_dir().join("modpack-base");
        std::fs::create_dir_all(&base).unwrap();
        Connection::open(base.join("Civ5DebugDatabase.db"))
            .unwrap()
            .execute_batch("CREATE TABLE Defines (Name text, Value integer);")
            .unwrap();
        Connection::open(base.join("Localization-Merged.db"))
            .unwrap()
            .execute_batch(
                "CREATE TABLE Language_en_US (Tag text PRIMARY KEY, Text text);
                 INSERT INTO Language_en_US VALUES ('TXT_KEY_BASE', 'Base text');
                 INSERT INTO Language_en_US VALUES ('TXT_KEY_VPUI_TITLE_TIP', 'Old tip title');",
            )
            .unwrap();
        // A stale local Text file must not supply the text for the selected Version.
        game.plant("Text/VPUI_tips_en_us.xml", "stale local text");
        let core = Core::new(
            Box::new(FixtureSourceProvider::new(miniature_repo())),
            Box::new(MarkerToolchainRunner),
            Box::new(SqliteModpackAssembler::new()),
            game.work_dir(),
        );
        let configuration = InstallConfiguration {
            source: InstallationSource::LocalRepo {
                path: miniature_repo(),
            },
            flavor: flavor.clone(),
            forty_three_civs: FortyThreeCivs::Disabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Modpack,
            extra_mods: Vec::new(),
            luajit: LuaJitEngine::Stock,
            menu_theme: MenuTheme::Stock,
            dll_source: DllSource::ShippedWhenCurrent,
        };
        let plan = core.plan(&configuration, &game.folders()).unwrap();
        core.execute(&plan, &ProgressReporter::silent()).unwrap();
        let dump_path = "DLC/VP_MODPACK/Override/CIV5Units_Mongol.xml";
        let first_dump = game.read(dump_path);
        assert!(first_dump.contains("<Text>\n\t\t\t\tBase text\n\t\t\t</Text>"));
        if matches!(flavor, Flavor::VoxPopuli { .. }) {
            for (tag, text) in [
                ("TXT_KEY_VPUI_TITLE_TIP", "Tip"),
                ("TXT_KEY_VPUI_TIP_26", "A fixture loading tip."),
                ("TXT_KEY_EUI_PROMOTION_FLAGS_DISPLAY", "Promotion Flags"),
            ] {
                assert!(
                    first_dump.contains(&format!(
                        "<Replace Tag=\"{tag}\">\n\t\t\t<Text>\n\t\t\t\t{text}\n\t\t\t</Text>"
                    )),
                    "{flavor:?}: missing {tag} in the distributed Modpack"
                );
            }
            assert!(!first_dump.contains("Old tip title"));
            assert!(!first_dump.contains("stale local text"));
        } else {
            assert!(!first_dump.contains("TXT_KEY_EUI_PROMOTION_FLAGS_DISPLAY"));
            assert!(!first_dump.contains("TXT_KEY_VPUI_TIP_26"));
        }
        // Recipients need only the DLC text dump, and rebuilding from an older saved base
        // must keep producing it even without any local Text file or game cache.
        std::fs::remove_dir_all(game.folders().text).unwrap();
        std::fs::create_dir_all(game.folders().text).unwrap();
        core.execute(&plan, &ProgressReporter::silent()).unwrap();
        assert_eq!(game.read(dump_path), first_dump);
    }
}
