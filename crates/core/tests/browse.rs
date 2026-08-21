//! Core-seam tests for where a `Browse` click opens the file browser.
//!
//! One test per rung of the ladder, per field, plus the two things that make the ladder safe:
//! a typed path that exists is never second-guessed, and only the detection rung ever writes
//! back into the box.
//!
//! Everything runs against fixture trees on Linux, including the home rung - home is an input
//! to the ladder, not something it reads from the environment, so no test here touches `HOME`.

#[path = "fixtures/steam.rs"]
mod steam;

use std::path::{Path, PathBuf};

use civ5vp_core::{BrowseField, BrowseRequest, BrowseStart, SearchLocations, browse_start};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn steam_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join("home/player/.local/share/Steam")
}

fn home(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("home/player");
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A request with every box empty and nowhere to look - each test fills in only the part it
/// is about, which is what keeps the rung under test the one that answered.
fn request<'a>(field: BrowseField, locations: &'a SearchLocations, home: &Path) -> Request<'a> {
    Request {
        field,
        game_folder: PathBuf::new(),
        documents_folder: PathBuf::new(),
        dev_checkout: PathBuf::new(),
        locations,
        home: home.to_path_buf(),
    }
}

/// Owns the paths a [`BrowseRequest`] borrows, so a test can build one field at a time.
struct Request<'a> {
    field: BrowseField,
    game_folder: PathBuf,
    documents_folder: PathBuf,
    dev_checkout: PathBuf,
    locations: &'a SearchLocations,
    home: PathBuf,
}

impl Request<'_> {
    fn start(&self) -> BrowseStart {
        browse_start(BrowseRequest {
            field: self.field,
            game_folder: &self.game_folder,
            documents_folder: &self.documents_folder,
            dev_checkout: &self.dev_checkout,
            locations: self.locations,
            home: Some(&self.home),
        })
    }
}

/// A Steam library with the Windows game and its Proton prefix, plus `libraryfolders.vdf`.
fn full_install(temp: &tempfile::TempDir) -> (SearchLocations, PathBuf, PathBuf) {
    let library = steam_root(temp);
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);
    steam::write_library_folders(&library, &[&library]);
    let locations = SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    };
    (locations, game, documents)
}

// --- Rung 1: what the box already holds ------------------------------------------------

/// A path that is there is the answer, for every field, and nothing is written back. This is
/// also what stops detection from ever clobbering a deliberate choice: rung 1 short-circuits
/// first, so rung 2 only ever fires on a path that is not there.
#[test]
fn a_folder_that_exists_is_where_the_browser_opens() {
    let temp = temp();
    let (locations, game, documents) = full_install(&temp);
    let checkout = temp.path().join("src/Community-Patch-DLL");
    std::fs::create_dir_all(&checkout).unwrap();
    let home = home(&temp);

    for (field, typed) in [
        (BrowseField::GameInstallation, &game),
        (BrowseField::Documents, &documents),
        (BrowseField::DevCheckout, &checkout),
    ] {
        let mut request = request(field, &locations, &home);
        request.game_folder = game.clone();
        request.documents_folder = documents.clone();
        request.dev_checkout = checkout.clone();
        // Every box is filled, so only the clicked field's box can be the answer.
        assert_eq!(
            request.start(),
            BrowseStart {
                directory: Some(typed.clone()),
                correction: None,
            },
            "{field:?} should have opened at its own box",
        );
    }
}

/// A folder the player typed that happens *not* to be the game is still where they were
/// looking. Rung 1 asks whether the path is there, not whether it is right - being wrong is
/// what the browser is for.
#[test]
fn a_folder_that_exists_but_is_not_the_game_is_still_where_the_browser_opens() {
    let temp = temp();
    let (locations, _, _) = full_install(&temp);
    let elsewhere = temp.path().join("Games/somewhere else");
    std::fs::create_dir_all(&elsewhere).unwrap();

    let mut request = request(BrowseField::GameInstallation, &locations, &home(&temp));
    request.game_folder = elsewhere.clone();

    assert_eq!(request.start().directory, Some(elsewhere));
}

// --- Rung 2: detection, which also corrects the box ------------------------------------

/// An empty game-folder box with the game detectable: the browser opens at the game, and the
/// box is filled in on the way, so cancelling still leaves the player better off.
#[test]
fn an_empty_game_folder_falls_back_to_detection_and_fills_the_box() {
    let temp = temp();
    let (locations, game, _) = full_install(&temp);

    let start = request(BrowseField::GameInstallation, &locations, &home(&temp)).start();

    assert_eq!(
        start,
        BrowseStart {
            directory: Some(game.clone()),
            correction: Some(game),
        }
    );
}

/// The same for the Documents side - the folder nine levels inside a Proton prefix that
/// nobody finds by hand.
#[test]
fn an_empty_documents_folder_falls_back_to_detection_and_fills_the_box() {
    let temp = temp();
    let (locations, _, documents) = full_install(&temp);

    let start = request(BrowseField::Documents, &locations, &home(&temp)).start();

    assert_eq!(
        start,
        BrowseStart {
            directory: Some(documents.clone()),
            correction: Some(documents),
        }
    );
}

/// A game found without its Documents side still answers the game-folder field: that is
/// exactly the machine where the player is about to correct the other box.
#[test]
fn a_game_found_without_its_documents_side_still_answers_the_game_field() {
    let temp = temp();
    let library = steam_root(&temp);
    let game = steam::install_windows_game(&library);
    steam::write_library_folders(&library, &[&library]);
    let locations = SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    };

    let start = request(BrowseField::GameInstallation, &locations, &home(&temp)).start();

    assert_eq!(start.directory, Some(game.clone()));
    assert_eq!(start.correction, Some(game));
}

/// The native Aspyr port is Civilization V and cannot be installed into. Detection refuses
/// it, so the ladder must not offer it - putting a known-bad path in the box would be worse
/// than leaving it empty.
#[test]
fn a_refused_game_is_never_written_into_the_box() {
    let temp = temp();
    let library = steam_root(&temp);
    steam::install_native_linux_port(&library);
    steam::write_library_folders(&library, &[&library]);
    let locations = SearchLocations {
        steam_roots: vec![library],
        documents_roots: Vec::new(),
    };
    let home = home(&temp);

    let start = request(BrowseField::GameInstallation, &locations, &home).start();

    assert_eq!(start.correction, None);
    assert_eq!(start.directory, Some(home));
}

/// Dev mode has no detection rung: a checkout can be anywhere, and there is nothing about
/// this game to detect. An empty box goes straight to home - and never to the Upstream
/// Cache, which is rewritten under the user.
#[test]
fn the_checkout_field_has_no_detection_rung() {
    let temp = temp();
    let (locations, _, _) = full_install(&temp);
    let home = home(&temp);

    let start = request(BrowseField::DevCheckout, &locations, &home).start();

    assert_eq!(
        start,
        BrowseStart {
            directory: Some(home),
            correction: None,
        }
    );
}

// --- Rung 3: the Documents side derived from the game folder ---------------------------

/// Detection cannot see this library - no `libraryfolders.vdf` names it - but the player has
/// already named the game folder, and the Documents side hangs off the same library. The
/// browser opens there, and nothing is written back: a starting point is not an answer.
#[test]
fn the_documents_folder_is_derived_from_the_game_folder_when_detection_finds_nothing() {
    let temp = temp();
    let library = temp.path().join("mnt/games/SteamLibrary");
    let game = steam::install_windows_game(&library);
    let documents = steam::create_proton_documents(&library);

    let nowhere = SearchLocations::default();
    let mut request = request(BrowseField::Documents, &nowhere, &home(&temp));
    request.game_folder = game;

    assert_eq!(
        request.start(),
        BrowseStart {
            directory: Some(documents),
            correction: None,
        }
    );
}

/// A prefix that stops part-way - Proton made one, but the game has never been run, so
/// `My Games` is not there yet. The browser opens at the deepest folder that does exist,
/// which is as far towards the answer as anything can take the player.
#[test]
fn a_half_built_proton_prefix_opens_at_its_deepest_real_folder() {
    let temp = temp();
    let library = temp.path().join("mnt/games/SteamLibrary");
    let game = steam::install_windows_game(&library);
    let deepest = library.join(format!(
        "steamapps/compatdata/{}/pfx/drive_c/users/steamuser/Documents",
        steam::APP_ID
    ));
    std::fs::create_dir_all(&deepest).unwrap();

    let nowhere = SearchLocations::default();
    let mut request = request(BrowseField::Documents, &nowhere, &home(&temp));
    request.game_folder = game;

    assert_eq!(request.start().directory, Some(deepest));
}

/// Detection wins over derivation: a machine where both could answer gets the folder that was
/// actually validated, not the one worked out from a path the player typed.
#[test]
fn detection_is_preferred_to_deriving_from_the_game_folder() {
    let temp = temp();
    let (locations, game, documents) = full_install(&temp);
    // A second library with its own prefix, which derivation would find first if it ran.
    let other = temp.path().join("mnt/games/SteamLibrary");
    let other_game = steam::install_windows_game(&other);
    steam::create_proton_documents(&other);

    let mut request = request(BrowseField::Documents, &locations, &home(&temp));
    request.game_folder = other_game;
    let start = request.start();

    assert_eq!(start.directory, Some(documents));
    assert!(game.exists());
}

/// A game folder with no prefix anywhere near it derives nothing, and the ladder falls
/// through to home rather than opening somewhere arbitrary up the tree.
#[test]
fn a_game_folder_with_no_prefix_beside_it_derives_nothing() {
    let temp = temp();
    let library = temp.path().join("mnt/games/SteamLibrary");
    let game = steam::install_windows_game(&library);
    let home = home(&temp);

    let nowhere = SearchLocations::default();
    let mut request = request(BrowseField::Documents, &nowhere, &home);
    request.game_folder = game;

    assert_eq!(request.start().directory, Some(home));
}

// --- Rung 4: home, and the floor below it ----------------------------------------------

/// Nothing typed, nothing detectable, nothing to derive: every field opens at home.
#[test]
fn everything_else_opens_at_home() {
    let temp = temp();
    let home = home(&temp);

    let nowhere = SearchLocations::default();
    for field in [
        BrowseField::GameInstallation,
        BrowseField::Documents,
        BrowseField::DevCheckout,
    ] {
        let start = request(field, &nowhere, &home).start();
        assert_eq!(
            start,
            BrowseStart {
                directory: Some(home.clone()),
                correction: None,
            },
            "{field:?} should have fallen through to home",
        );
    }
}

/// A platform that cannot even say where home is says so, rather than guessing. The browser
/// then opens wherever it would have anyway.
#[test]
fn a_machine_without_a_home_directory_names_no_starting_point() {
    let locations = SearchLocations::default();
    let start = browse_start(BrowseRequest {
        field: BrowseField::DevCheckout,
        game_folder: Path::new(""),
        documents_folder: Path::new(""),
        dev_checkout: Path::new(""),
        locations: &locations,
        home: None,
    });

    assert_eq!(
        start,
        BrowseStart {
            directory: None,
            correction: None,
        }
    );
}
