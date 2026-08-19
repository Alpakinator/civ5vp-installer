//! Core-seam tests for finding the game and for judging a folder the user picked.
//!
//! Everything here runs on Linux against fixture trees, including the Windows arrangement:
//! detection takes the directories to look in as input, and the only platform-specific step
//! is producing that list. So the decision logic - is this the
//! Windows game? the native port? the Documents side? - is exercised on both layouts by the
//! same tests, on the one machine this project is verified on.

#[path = "fixtures/steam.rs"]
mod steam;

use std::path::{Path, PathBuf};

use civ5vp_core::{
    Detection, RejectionReason, SearchLocations, detect_game, resolve_game_folders,
    validate_documents_folder, validate_game_installation,
};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// A Steam installation root, with `steamapps/` inside it, in a temporary directory.
fn steam_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join("home/player/.local/share/Steam")
}

fn found(detection: Detection) -> civ5vp_core::DetectedGame {
    match detection {
        Detection::Found(game) => game,
        other => panic!("expected a complete Game Installation, got {other:?}"),
    }
}

/// The reference layout: game and Proton prefix in the same Steam library.
#[test]
fn linux_detection_resolves_the_game_and_the_proton_documents_folder() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);
    steam::write_library_folders(&library, &[&library]);

    let detection = detect_game(&SearchLocations {
        steam_roots: vec![library.clone()],
        documents_roots: Vec::new(),
    });

    let detected = found(detection);
    assert_eq!(detected.game_installation.root(), game);
    assert_eq!(detected.documents.root(), documents);

    let folders = detected.folders();
    assert_eq!(folders.mods, documents.join("MODS"));
    assert_eq!(folders.text, documents.join("Text"));
    assert_eq!(folders.dlc, game.join("Assets/DLC"));
}

/// Steam libraries live on other drives. Every one named in `libraryfolders.vdf` is searched.
#[test]
fn every_library_named_in_libraryfolders_vdf_is_searched() {
    let temp = temp();
    let library = steam_root(&temp);
    let second = temp.path().join("mnt/games/SteamLibrary");
    let third = temp.path().join("mnt/spinning-rust/SteamLibrary");
    std::fs::create_dir_all(&second).unwrap();
    // The game is in the last library listed, and only that one has it.
    let game = steam::install_windows_game(&third);
    let documents = steam::create_proton_documents(&third);
    steam::write_library_folders(&library, &[&library, &second, &third]);

    let detected = found(detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    }));

    assert_eq!(detected.game_installation.root(), game);
    assert_eq!(detected.documents.root(), documents);
}

/// Steam's older `"1" "<path>"` shape is still on plenty of machines.
#[test]
fn the_old_style_libraryfolders_file_is_understood() {
    let temp = temp();
    let library = steam_root(&temp);
    let second = temp.path().join("mnt/games/SteamLibrary");
    let game = steam::install_windows_game(&second);
    steam::create_proton_documents(&second);
    steam::write_old_style_library_folders(&library, &[&second]);

    let detected = found(detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    }));

    assert_eq!(detected.game_installation.root(), game);
}

/// The Documents folder says "Civilization 5"; the game folder says "Civilization V". They are
/// two different names, and neither is derived from the other.
///
/// Both decoys here are complete and valid *as folders* - they carry every marker. The only
/// thing that tells them apart from the real ones is the name, so a detector that substituted
/// one numeral for the other would return a decoy and this test would catch it.
#[test]
fn the_two_folder_names_are_never_derived_from_each_other() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);

    // A game folder named like the Documents folder, and a Documents folder named like the
    // game folder - both fully marked, both wrong.
    let game_decoy = library
        .join("steamapps/common")
        .join(steam::DOCUMENTS_FOLDER);
    std::fs::create_dir_all(game_decoy.parent().unwrap()).unwrap();
    copy_tree(&game, &game_decoy);
    let documents_decoy = documents.parent().unwrap().join(steam::GAME_FOLDER);
    copy_tree(&documents, &documents_decoy);

    let detected = found(detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    }));

    assert_eq!(
        detected.game_installation.root(),
        game,
        "the Game Installation is the \"Civilization V\" folder",
    );
    assert_eq!(
        detected.documents.root(),
        documents,
        "the Documents side is the \"Civilization 5\" folder",
    );
}

/// The native Aspyr port is found only so that it can be refused, in words a player can
/// act on.
#[test]
fn the_native_linux_port_is_detected_and_refused_with_an_explanation() {
    let temp = temp();
    let library = steam_root(&temp);
    let port = steam::install_native_linux_port(&library);
    steam::create_proton_documents(&library);

    let detection = detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    });

    let Detection::Refused(rejected) = &detection else {
        panic!("the native port should be refused, got {detection:?}");
    };
    assert_eq!(rejected.reason, RejectionReason::NativeLinuxPort);
    assert_eq!(rejected.path, port);

    let message = detection.user_message().expect("a refusal explains itself");
    assert!(
        message.contains("Proton") && message.contains("Windows"),
        "the explanation should name Proton and the Windows version, got: {message}",
    );

    // And there is no way to deploy against it: detection hands back no folders at all.
    assert!(matches!(detection, Detection::Refused(_)));
    assert!(validate_game_installation(&port).is_err());
}

/// Vox Populi needs Brave New World, so a game without it is refused rather than warned about.
#[test]
fn a_game_without_brave_new_world_is_refused() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_game_without_brave_new_world(&library);
    steam::create_proton_documents(&library);

    let detection = detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    });

    let Detection::Refused(rejected) = &detection else {
        panic!("a game without Brave New World should be refused, got {detection:?}");
    };
    assert_eq!(rejected.reason, RejectionReason::BraveNewWorldMissing);
    assert_eq!(rejected.path, game);
    assert!(
        rejected.user_message().contains("Brave New World"),
        "got: {}",
        rejected.user_message(),
    );
}

/// The Windows arrangement: the same Steam libraries, but the Documents side sits in the
/// user's profile rather than in a Proton prefix. The adapter produces that candidate; the
/// logic under test is the same one Linux uses, which is why it can be checked here.
#[test]
fn the_windows_arrangement_resolves_documents_from_the_user_profile() {
    let temp = temp();
    let library = temp.path().join("C_/Program Files (x86)/Steam");
    let game = steam::install_windows_game(&library);
    let profile = temp.path().join("C_/Users/Player/Documents");
    let documents = steam::create_documents(&profile);
    steam::write_library_folders(&library, &[&library]);

    let detected = found(detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: vec![profile],
    }));

    assert_eq!(detected.game_installation.root(), game);
    assert_eq!(detected.documents.root(), documents);
    assert_eq!(detected.folders().mods, documents.join("MODS"));
}

/// A game that has never been launched has no Documents side yet. That is not a refusal - the
/// game folder is still worth handing back - but it is not a complete answer either.
#[test]
fn a_game_that_has_never_been_launched_reports_the_missing_documents_folder() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_windows_game(&library);

    let detection = detect_game(&SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    });

    let Detection::DocumentsNotFound {
        game_installation, ..
    } = &detection
    else {
        panic!("expected the game without its Documents side, got {detection:?}");
    };
    assert_eq!(game_installation.root(), game);
    assert!(
        detection.user_message().is_some(),
        "the player needs to be told what to do next",
    );
}

/// Nothing installed anywhere.
#[test]
fn an_empty_machine_reports_that_nothing_was_found() {
    let temp = temp();
    let detection = detect_game(&SearchLocations {
        steam_roots: vec![steam_root(&temp)],
        documents_roots: Vec::new(),
    });

    let Detection::NotFound { searched } = &detection else {
        panic!("expected nothing to be found, got {detection:?}");
    };
    assert!(!searched.is_empty(), "the log should say where we looked");
    assert!(detection.user_message().is_some());
}

/// A wrong folder is caught before anything is written, and the message names
/// the marker that was missing so the player can tell which folder was wanted.
#[test]
fn a_picked_game_folder_is_rejected_naming_the_marker_that_is_missing() {
    let temp = temp();
    let library = steam_root(&temp);

    for marker in ["CivilizationV.exe", "CivilizationV_DX11.exe", "Assets/DLC"] {
        let library = library.join(marker.replace('/', "-"));
        let game = steam::install_windows_game(&library);
        let path = game.join(marker);
        if path.is_dir() {
            std::fs::remove_dir_all(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }

        let rejected = validate_game_installation(&game)
            .expect_err("a folder missing a marker is not the game");
        assert_eq!(
            rejected.reason,
            RejectionReason::MissingMarker { marker },
            "removing {marker} should be reported as {marker} missing",
        );
        assert!(
            rejected.user_message().contains(marker),
            "the message should name {marker}, got: {}",
            rejected.user_message(),
        );
    }
}

#[test]
fn a_picked_documents_folder_is_rejected_naming_the_marker_that_is_missing() {
    let temp = temp();
    for marker in ["MODS", "Text", "ModUserData", "UserSettings.ini"] {
        let documents = steam::create_documents_without(&temp.path().join(marker), marker);

        let rejected = validate_documents_folder(&documents)
            .expect_err("a folder missing a marker is not the Documents folder");
        assert_eq!(rejected.reason, RejectionReason::MissingMarker { marker });
        assert!(rejected.user_message().contains(marker));
    }
}

/// The other ways a picked path can be unusable, all caught before a Deployment is planned.
#[test]
fn a_picked_folder_that_is_not_a_real_absolute_directory_is_rejected() {
    let temp = temp();
    let cases = [
        (PathBuf::new(), RejectionReason::NotChosen),
        (
            PathBuf::from("Sid Meier's Civilization V"),
            RejectionReason::NotAbsolute,
        ),
        (
            temp.path().join("nothing-here"),
            RejectionReason::NotADirectory,
        ),
    ];
    for (path, expected) in cases {
        let rejected = validate_game_installation(&path).expect_err("{path:?} is not the game");
        assert_eq!(rejected.reason, expected);
        assert!(!rejected.user_message().is_empty());
    }
}

/// The manual picker's whole job in one call: two folders in, three Deployment targets out.
#[test]
fn picked_folders_resolve_to_the_three_deployment_targets() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);

    let folders = resolve_game_folders(&game, &documents).expect("both folders are the real thing");

    assert_eq!(folders.mods, documents.join("MODS"));
    assert_eq!(folders.text, documents.join("Text"));
    assert_eq!(folders.dlc, game.join("Assets/DLC"));

    // Swapping the two is caught, rather than producing paths that point nowhere.
    let rejected = resolve_game_folders(&documents, &game).expect_err("the folders are swapped");
    assert!(matches!(
        rejected.reason,
        RejectionReason::MissingMarker { .. }
    ));
}

/// Detection against the machine this is running on, rather than against a fixture.
///
/// `#[ignore]`d because it depends on the developer having Civilization V installed, which the
/// fast suite must not. It is the only check that the platform adapter's guesses at
/// where Steam lives are right, so it is kept rather than run once and thrown away:
///
/// ```text
/// cargo test -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs Civilization V installed on this machine"]
fn detection_finds_the_game_on_this_machine() {
    let detection = detect_game(&SearchLocations::for_this_platform());
    println!("{detection:#?}");
    let game = found(detection);
    println!("{:#?}", game.folders());
}

/// A Proton prefix mirrors a Windows tree, where case never mattered. Folder names inside one
/// are therefore matched the way Windows would match them.
#[test]
fn folder_names_are_matched_regardless_of_case() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);
    std::fs::rename(documents.join("MODS"), documents.join("Mods")).unwrap();

    let folders = resolve_game_folders(&game, &documents).expect("case is not a difference");

    // The property that matters on every platform: the resolved path opens the folder that is
    // really there. Written through the name on disk, read back through the resolved one.
    std::fs::write(documents.join("Mods").join("marker.txt"), b"here").unwrap();
    assert!(
        folders.mods.join("marker.txt").is_file(),
        "the resolved path must open the folder on disk, got {}",
        folders.mods.display(),
    );

    // The *spelling* is only observable where the filesystem is case-sensitive. On Windows
    // `locate` finds `MODS` exists - it is the same directory as `Mods` there - takes the
    // exact-match fast path, and never scans for the on-disk name. Nothing depends on which
    // of the two it hands back, because both open the same folder.
    #[cfg(unix)]
    assert_eq!(
        folders.mods,
        documents.join("Mods"),
        "the resolved path is the one that is really on disk",
    );
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let destination = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}
