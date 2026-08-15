//! Steam library trees, built in a temporary directory.
//!
//! Built in code rather than committed, because the interesting parts of a Civilization V
//! install are empty marker directories (`Assets/DLC/Expansion2/`) and zero-byte executables
//! — neither of which git can carry. Keeping every layout in one file also makes the
//! differences between them (native port, no Brave New World, Proton prefix vs Windows user
//! profile) readable side by side.
//!
//! Included with `#[path]` by the test files that need it, the way `tests/support/mod.rs` is
//! included by `tests/deployment.rs`.

// Each test file uses a different subset of the layouts.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// Steam's app id for Civilization V.
pub const APP_ID: &str = "8930";

/// The Game Installation's folder name in a Steam library. Roman numeral.
pub const GAME_FOLDER: &str = "Sid Meier's Civilization V";

/// The Documents side's folder name. Arabic numeral — a different name, not a different
/// spelling of the same one.
pub const DOCUMENTS_FOLDER: &str = "Sid Meier's Civilization 5";

fn directory(path: PathBuf) -> PathBuf {
    fs::create_dir_all(&path).unwrap();
    path
}

fn file(path: PathBuf, contents: &str) -> PathBuf {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// A complete, usable Game Installation: the Steam install of the *Windows* game, with Brave
/// New World. Returns the Game Installation root.
pub fn install_windows_game(library: &Path) -> PathBuf {
    let root = directory(library.join("steamapps/common").join(GAME_FOLDER));
    file(root.join("CivilizationV.exe"), "not really an executable");
    file(
        root.join("CivilizationV_DX11.exe"),
        "not really an executable",
    );
    directory(root.join("Assets/DLC/Expansion2"));
    // Not a marker, but every real install has more DLC than BNW — present so a test that
    // asserts on the DLC Folder is looking at something realistic.
    directory(root.join("Assets/DLC/Expansion"));
    root
}

/// The same, minus Brave New World.
pub fn install_game_without_brave_new_world(library: &Path) -> PathBuf {
    let root = install_windows_game(library);
    fs::remove_dir_all(root.join("Assets/DLC/Expansion2")).unwrap();
    root
}

/// The native Aspyr Linux port: a real Civilization V, in the same place in the library, that
/// cannot load the Built DLL.
pub fn install_native_linux_port(library: &Path) -> PathBuf {
    let root = directory(library.join("steamapps/common").join(GAME_FOLDER));
    file(root.join("Civ5XP"), "the Aspyr port's binary");
    file(root.join("civ5"), "the Aspyr port's launcher script");
    directory(root.join("steamassets/Assets/DLC/Expansion2"));
    root
}

/// The Documents side inside a Proton prefix, as it exists on Linux. Returns its root.
pub fn create_proton_documents(library: &Path) -> PathBuf {
    let prefix = library
        .join("steamapps/compatdata")
        .join(APP_ID)
        .join("pfx/drive_c/users/steamuser/Documents");
    create_documents(&prefix)
}

/// The Documents side under a Windows user profile's Documents folder: this is what the
/// Windows adapter's candidates lead to. Returns its root.
pub fn create_documents(documents_root: &Path) -> PathBuf {
    let root = directory(documents_root.join("My Games").join(DOCUMENTS_FOLDER));
    directory(root.join("MODS"));
    directory(root.join("Text"));
    directory(root.join("ModUserData"));
    file(root.join("UserSettings.ini"), "[Game]\n");
    root
}

/// A Documents side with one marker taken away, so a test can name what is missing.
pub fn create_documents_without(documents_root: &Path, marker: &str) -> PathBuf {
    let root = create_documents(documents_root);
    let path = root.join(marker);
    if path.is_dir() {
        fs::remove_dir_all(path).unwrap();
    } else {
        fs::remove_file(path).unwrap();
    }
    root
}

/// Write `steamapps/libraryfolders.vdf` in the shape Steam writes today: one numbered block
/// per library, each with a `path`, and the game's app id listed under `apps`.
pub fn write_library_folders(steam_root: &Path, libraries: &[&Path]) {
    let mut text = String::from("\"libraryfolders\"\n{\n");
    for (index, library) in libraries.iter().enumerate() {
        text.push_str(&format!(
            "\t\"{index}\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t\t\"label\"\t\t\"\"\n\
             \t\t\"contentid\"\t\t\"1234567890\"\n\t\t\"apps\"\n\t\t{{\n\
             \t\t\t\"{APP_ID}\"\t\t\"9876543210\"\n\t\t}}\n\t}}\n",
            escape(library),
        ));
    }
    text.push_str("}\n");
    file(steam_root.join("steamapps/libraryfolders.vdf"), &text);
}

/// The same file in the shape Steam used to write: `"1" "<path>"` and nothing else.
pub fn write_old_style_library_folders(steam_root: &Path, libraries: &[&Path]) {
    let mut text =
        String::from("\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\"1700000000\"\n");
    for (index, library) in libraries.iter().enumerate() {
        text.push_str(&format!("\t\"{}\"\t\t\"{}\"\n", index + 1, escape(library)));
    }
    text.push_str("}\n");
    file(steam_root.join("steamapps/libraryfolders.vdf"), &text);
}

/// VDF escapes backslashes, which is how Windows paths survive the round trip.
fn escape(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}
