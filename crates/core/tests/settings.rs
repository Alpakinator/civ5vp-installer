//! Core-seam tests for what the installer remembers between runs, and where it keeps it.

#[path = "fixtures/steam.rs"]
mod steam;

use std::path::PathBuf;

use civ5vp_core::{
    AppDataStore, BuildConfiguration, Eui, Flavor, FortyThreeCivs, InstallConfiguration,
    InstallMode, InstallationSource, LuaJitEngine, SearchLocations, Settings, Version, start_up,
};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// A store in a temporary directory, so no test touches the real App Data Store.
fn store(temp: &tempfile::TempDir) -> AppDataStore {
    AppDataStore::at(temp.path().join("app-data"))
}

/// Nowhere to search: every start-up test that is about remembering, rather than about
/// finding, uses this so the machine running the tests is never consulted.
fn nowhere() -> SearchLocations {
    SearchLocations::default()
}

/// The store answers "how big" and "make it go away", and clearing leaves a
/// state a next install starts cleanly from.
#[test]
fn the_store_reports_its_size_and_clears_to_nothing() {
    let temp = temp();
    let store = store(&temp);
    assert_eq!(store.size_on_disk(), 0, "an unwritten store is zero bytes");
    store.save(&Settings::default()).unwrap();
    // Something cache-shaped beside the settings, like the real store holds.
    let cache = store.root().join("toolchain-cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("blob"), vec![7u8; 4096]).unwrap();

    let size = store.size_on_disk();
    assert!(size >= 4096, "the cache bytes must be counted, got {size}");

    store.clear().unwrap();
    assert_eq!(store.size_on_disk(), 0);
    assert_eq!(
        std::fs::read_dir(store.root()).unwrap().count(),
        0,
        "the store is emptied, the directory itself stays"
    );
    // A cleared store reads back as a first run, not an error.
    assert_eq!(store.load().unwrap(), Settings::default());
    // Clearing twice, or clearing a store that never existed, is fine.
    store.clear().unwrap();
    AppDataStore::at(temp.path().join("never-created"))
        .clear()
        .unwrap();
}

#[test]
fn a_first_run_has_nothing_remembered() {
    let temp = temp();
    let settings = store(&temp).load().expect("a missing file is a first run");
    assert_eq!(settings, Settings::default());
}

/// Every field of the remembered state survives the round trip, for every shape an Install
/// Configuration can take.
#[test]
fn the_remembered_state_survives_the_round_trip() {
    let temp = temp();
    let store = store(&temp);

    let configurations = [
        InstallConfiguration {
            source: InstallationSource::LocalRepo {
                path: PathBuf::from("/home/player/src/Community-Patch-DLL"),
            },
            flavor: Flavor::CommunityPatch,
            forty_three_civs: FortyThreeCivs::Disabled,
            // Debug in the round-trip: the Dev-mode choice must survive a relaunch too.
            build_configuration: BuildConfiguration::Debug,
            install_mode: InstallMode::Mods,
            extra_mods: Vec::new(),
            luajit: LuaJitEngine::Stock,
        },
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::Release("Release-4.19.1".to_owned()),
            },
            flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
            forty_three_civs: FortyThreeCivs::Enabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Mods,
            extra_mods: Vec::new(),
            // LuaJIT in the round-trip: a choice that replaces a file belonging to the game
            // must survive a relaunch, or the next Deployment would quietly revert it.
            luajit: LuaJitEngine::LuaJit,
        },
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::LatestDevelopmentVersion,
            },
            flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
            forty_three_civs: FortyThreeCivs::Disabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Mods,
            extra_mods: Vec::new(),
            luajit: LuaJitEngine::Stock,
        },
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::ArbitraryRef("d34db33f".to_owned()),
            },
            flavor: Flavor::CommunityPatch,
            forty_three_civs: FortyThreeCivs::Enabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Mods,
            extra_mods: Vec::new(),
            luajit: LuaJitEngine::Stock,
        },
        // An unofficial build: the label and the commit both survive.
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::UnofficialBuild {
                    label: "5.4.3.07".to_owned(),
                    commit: "a".repeat(40),
                },
            },
            flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
            forty_three_civs: FortyThreeCivs::Disabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Mods,
            extra_mods: Vec::new(),
            luajit: LuaJitEngine::Stock,
        },
        // Modpack mode with extra picks — names with spaces and parentheses, like real
        // mod folders have.
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::Release("Release-4.19.1".to_owned()),
            },
            flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
            forty_three_civs: FortyThreeCivs::Disabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Modpack,
            extra_mods: vec!["Even More Bonuses (v 3)".to_owned(), "My Modmod".to_owned()],
            luajit: LuaJitEngine::Stock,
        },
    ];

    for configuration in configurations {
        let settings = Settings {
            game_installation: Some(PathBuf::from("/games/Sid Meier's Civilization V")),
            documents_folder: Some(PathBuf::from("/prefix/My Games/Sid Meier's Civilization 5")),
            configuration: Some(configuration),
            dev_checkout: None,
        };
        store.save(&settings).expect("the store is writable");
        assert_eq!(store.load().expect("just written"), settings);
    }
}

/// The settings file is the installer's own, in the App Data Store, and it is plain text a
/// person can read.
#[test]
fn the_settings_file_lives_in_the_app_data_store() {
    let temp = temp();
    let store = store(&temp);
    store
        .save(&Settings {
            game_installation: Some(PathBuf::from("/games/Sid Meier's Civilization V")),
            ..Settings::default()
        })
        .unwrap();

    let file = store.settings_file();
    assert!(
        file.starts_with(store.root()),
        "{file:?} is not in the store"
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("/games/Sid Meier's Civilization V"), "{text}");
}

/// A settings file from another version, or one somebody edited, must not stop the installer
/// from starting.
#[test]
fn unreadable_settings_do_not_stop_the_installer() {
    let temp = temp();
    let store = store(&temp);
    std::fs::create_dir_all(store.root()).unwrap();
    std::fs::write(
        store.settings_file(),
        "# hand-edited\nsomething-from-a-later-version = yes\nnot a key value line\n\
         game-installation = /games/Sid Meier's Civilization V\n",
    )
    .unwrap();

    let settings = store.load().expect("unknown lines are ignored, not fatal");
    assert_eq!(
        settings.game_installation,
        Some(PathBuf::from("/games/Sid Meier's Civilization V")),
    );
    assert_eq!(settings.configuration, None);
}

/// The folders and the last configuration pre-fill the next launch.
#[test]
fn remembered_folders_pre_fill_the_next_launch() {
    let temp = temp();
    let store = store(&temp);
    let library = temp.path().join("Steam");
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);
    let configuration = InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: PathBuf::from("/home/player/src/Community-Patch-DLL"),
        },
        flavor: Flavor::CommunityPatch,
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
    };

    store
        .save(&Settings {
            game_installation: Some(game.clone()),
            documents_folder: Some(documents.clone()),
            configuration: Some(configuration.clone()),
            dev_checkout: None,
        })
        .unwrap();

    // Nowhere to search: what comes back can only have come from the store.
    let startup = start_up(&store, &nowhere());

    assert_eq!(startup.game_installation, Some(game));
    assert_eq!(startup.documents_folder, Some(documents));
    assert_eq!(startup.configuration, Some(configuration));
    assert_eq!(
        startup.note, None,
        "nothing to explain: the folders are there"
    );
}

/// Nothing remembered yet — the first launch on a machine finds the game itself.
#[test]
fn a_first_launch_falls_back_to_detection() {
    let temp = temp();
    let library = temp.path().join("Steam");
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);
    steam::write_library_folders(&library, &[&library]);

    let startup = start_up(
        &store(&temp),
        &SearchLocations {
            steam_roots: vec![library],
            documents_roots: Vec::new(),
        },
    );

    assert_eq!(startup.game_installation, Some(game));
    assert_eq!(startup.documents_folder, Some(documents));
    assert_eq!(startup.note, None);
}

/// The player moved their Steam library. What was remembered no longer validates, so
/// detection gets the last word.
#[test]
fn remembered_folders_that_are_gone_give_way_to_detection() {
    let temp = temp();
    let store = store(&temp);
    let library = temp.path().join("Steam");
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);
    steam::write_library_folders(&library, &[&library]);

    store
        .save(&Settings {
            game_installation: Some(temp.path().join("an-old-drive/Sid Meier's Civilization V")),
            documents_folder: Some(temp.path().join("an-old-drive/Sid Meier's Civilization 5")),
            configuration: None,
            dev_checkout: None,
        })
        .unwrap();

    let startup = start_up(
        &store,
        &SearchLocations {
            steam_roots: vec![library],
            documents_roots: Vec::new(),
        },
    );

    assert_eq!(startup.game_installation, Some(game));
    assert_eq!(startup.documents_folder, Some(documents));
    assert!(
        !startup.log.is_empty(),
        "why the remembered folders were dropped belongs in the log (rule 11)",
    );
}

/// Nothing remembered and nothing found: the launch still produces something the player can
/// act on, rather than an empty window.
#[test]
fn a_launch_with_nothing_to_go_on_explains_itself() {
    let temp = temp();
    let startup = start_up(&store(&temp), &nowhere());

    assert_eq!(startup.game_installation, None);
    assert_eq!(startup.documents_folder, None);
    let note = startup
        .note
        .expect("the player needs to be told what to do");
    assert!(note.contains("Civilization V"), "got: {note}");
}

/// The native port refusal survives the trip through start-up: it is what the player sees.
#[test]
fn a_native_port_refusal_reaches_the_launch_note() {
    let temp = temp();
    let library = temp.path().join("Steam");
    steam::install_native_linux_port(&library);
    steam::create_proton_documents(&library);

    let startup = start_up(
        &store(&temp),
        &SearchLocations {
            steam_roots: vec![library],
            documents_roots: Vec::new(),
        },
    );

    let note = startup.note.expect("a refusal explains itself");
    assert!(note.contains("Proton"), "got: {note}");
    assert_eq!(
        startup.game_installation, None,
        "there is nothing to install into"
    );
}

/// The App Data Store is the platform's app-data location, not somewhere next to the exe.
#[test]
fn the_platform_app_data_store_is_the_platform_app_data_location() {
    let store = AppDataStore::for_this_platform().expect("this machine has an app-data location");
    assert!(store.root().is_absolute(), "{:?}", store.root());

    #[cfg(unix)]
    {
        let expected = match std::env::var_os("XDG_DATA_HOME") {
            Some(xdg) if !xdg.is_empty() => PathBuf::from(xdg),
            _ => PathBuf::from(std::env::var_os("HOME").expect("HOME is set")).join(".local/share"),
        };
        assert_eq!(store.root(), expected.join("civ5vp-installer"));
    }
}

/// A Flavor chosen before the sources have been pointed at anything is still remembered.
///
/// The Install Configuration is otherwise remembered whole or not at all, which is right for
/// everything except the Installation Source: on a first run nobody has named one yet, and
/// dropping the whole configuration over that would mean the Flavor silently never persisted.
#[test]
fn a_configuration_with_no_installation_source_still_remembers_its_flavor() {
    let store = AppDataStore::at(tempfile::tempdir().unwrap().path().join("app-data"));

    store
        .save(&Settings {
            game_installation: None,
            documents_folder: None,
            configuration: Some(InstallConfiguration {
                source: InstallationSource::unchosen(),
                flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
                forty_three_civs: FortyThreeCivs::Enabled,
                build_configuration: BuildConfiguration::Release,
                install_mode: InstallMode::Mods,
                extra_mods: Vec::new(),
                luajit: LuaJitEngine::Stock,
            }),
            dev_checkout: None,
        })
        .unwrap();

    let restored = store
        .load()
        .unwrap()
        .configuration
        .expect("the Flavor and the toggles should have survived on their own");
    assert_eq!(restored.flavor, Flavor::VoxPopuli { eui: Eui::Enabled });
    assert_eq!(restored.forty_three_civs, FortyThreeCivs::Enabled);
    assert_eq!(restored.source, InstallationSource::unchosen());
}

/// A remembered Version survives a run that never touches the source, so that a build with no
/// Version picker cannot quietly throw one away.
#[test]
fn a_remembered_upstream_version_survives_a_round_trip() {
    let store = AppDataStore::at(tempfile::tempdir().unwrap().path().join("app-data"));
    let chosen = InstallationSource::UpstreamCache {
        version: Version::Release("Release-5.4.2".to_owned()),
    };

    store
        .save(&Settings {
            game_installation: None,
            documents_folder: None,
            configuration: Some(InstallConfiguration {
                source: chosen.clone(),
                flavor: Flavor::CommunityPatch,
                forty_three_civs: FortyThreeCivs::Disabled,
                build_configuration: BuildConfiguration::Release,
                install_mode: InstallMode::Mods,
                extra_mods: Vec::new(),
                luajit: LuaJitEngine::Stock,
            }),
            dev_checkout: None,
        })
        .unwrap();

    assert_eq!(
        store.load().unwrap().configuration.unwrap().source,
        chosen,
        "the remembered Version should come back exactly",
    );
}

/// The Dev-mode checkout is remembered on its own, not inside the configuration: naming it
/// once, then installing from GitHub, must not cost the player the path.
#[test]
fn the_dev_checkout_outlives_a_switch_back_to_github() {
    let store = AppDataStore::at(tempfile::tempdir().unwrap().path().join("app-data"));
    store
        .save(&Settings {
            game_installation: None,
            documents_folder: None,
            // The active source is GitHub — the checkout is not part of the configuration.
            configuration: Some(InstallConfiguration {
                source: InstallationSource::UpstreamCache {
                    version: Version::Release("Release-5.4.3".to_owned()),
                },
                flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
                forty_three_civs: FortyThreeCivs::Disabled,
                build_configuration: BuildConfiguration::Release,
                install_mode: InstallMode::Mods,
                extra_mods: Vec::new(),
                luajit: LuaJitEngine::Stock,
            }),
            dev_checkout: Some(PathBuf::from("/home/player/src/Community-Patch-DLL")),
        })
        .unwrap();

    let startup = start_up(&store, &nowhere());

    assert_eq!(
        startup.dev_checkout,
        Some(PathBuf::from("/home/player/src/Community-Patch-DLL")),
    );
}

/// A key this build has never heard of survives a save.
///
/// The real case: a player keeps two installer versions on one machine and runs the older
/// one. It rewrites the settings whole from the fields it knows, and every choice only the
/// newer build understands would otherwise vanish — silently, and invisibly until they go
/// looking for the setting again.
#[test]
fn a_setting_this_build_does_not_understand_survives_being_rewritten() {
    let temp = temp();
    let store = store(&temp);

    // Written by some future build: one key this one knows, one it does not.
    store.save(&Settings::default()).unwrap();
    let path = store.settings_file();
    let seeded = std::fs::read_to_string(&path).unwrap()
        + "\nsomething-from-the-future = keep me\ngame-installation = /somewhere\n";
    std::fs::write(&path, seeded).unwrap();

    store
        .save(&Settings {
            game_installation: Some(PathBuf::from("/elsewhere")),
            ..Settings::default()
        })
        .unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("something-from-the-future = keep me"),
        "an unknown key must be carried across, got:\n{written}"
    );
    assert!(
        written.contains("game-installation = /elsewhere"),
        "a known key must still be rewritten from this build's own state, got:\n{written}"
    );
    assert!(
        !written.contains("/somewhere"),
        "the old value of a known key must not survive, got:\n{written}"
    );
}

/// A key this build knows but did not write on this run must not come back from the old
/// file. `version` belongs to a configuration; with no configuration there is no version,
/// and resurrecting the previous one would install something nobody chose.
#[test]
fn a_known_key_left_out_this_time_is_not_resurrected() {
    let temp = temp();
    let store = store(&temp);
    let path = store.settings_file();

    store
        .save(&Settings {
            configuration: Some(InstallConfiguration {
                source: InstallationSource::UpstreamCache {
                    version: Version::Release("Release-4.0".to_owned()),
                },
                flavor: Flavor::CommunityPatch,
                forty_three_civs: FortyThreeCivs::Disabled,
                build_configuration: BuildConfiguration::Release,
                install_mode: InstallMode::Mods,
                extra_mods: Vec::new(),
                luajit: LuaJitEngine::Stock,
            }),
            ..Settings::default()
        })
        .unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("Release-4.0")
    );

    store.save(&Settings::default()).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        !written.contains("Release-4.0"),
        "a known key this build chose not to write must stay gone, got:\n{written}"
    );
}
