//! Core-seam tests for what the installer remembers between runs, and where it keeps it.

#[path = "fixtures/steam.rs"]
mod steam;

use std::path::PathBuf;

use civ5vp_core::{
    AppDataStore, Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallationSource,
    SearchLocations, Settings, Version, start_up,
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
        },
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::Release("Release-4.19.1".to_owned()),
            },
            flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
            forty_three_civs: FortyThreeCivs::Enabled,
        },
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::LatestDevelopmentVersion,
            },
            flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
            forty_three_civs: FortyThreeCivs::Disabled,
        },
        InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::ArbitraryRef("d34db33f".to_owned()),
            },
            flavor: Flavor::CommunityPatch,
            forty_three_civs: FortyThreeCivs::Enabled,
        },
    ];

    for configuration in configurations {
        let settings = Settings {
            game_installation: Some(PathBuf::from("/games/Sid Meier's Civilization V")),
            documents_folder: Some(PathBuf::from("/prefix/My Games/Sid Meier's Civilization 5")),
            configuration: Some(configuration),
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

/// User story 26: the folders and the last configuration pre-fill the next launch.
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
    };

    store
        .save(&Settings {
            game_installation: Some(game.clone()),
            documents_folder: Some(documents.clone()),
            configuration: Some(configuration.clone()),
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
