//! Opening a folder in the machine's own file manager.
//!
//! The installer knows where three folders are - the game, the Documents folder, a
//! developer's checkout - and a player who wants to look inside one has to find it again by
//! hand. On Linux that is a genuine chore: the Documents folder sits nine levels inside a
//! Proton prefix, under a path nobody types twice.
//!
//! This is a one-way door: the file manager is handed a folder and the installer hears
//! nothing back. Nothing about a Deployment depends on it, so a failure is logged and
//! forgotten rather than shown - a player who clicked and saw no window will click again,
//! and a dialog explaining `xdg-open` would help nobody.

use std::path::Path;
use std::process::{Command, Stdio};

/// Open `folder` in the file manager, if it is there.
///
/// Returns whether the command could be started at all - not whether a window appeared,
/// which nothing here can know.
pub fn folder(folder: &Path) -> bool {
    if !folder.is_dir() {
        crate::log_detail(&format!(
            "not opening {}: no such folder",
            folder.display()
        ));
        return false;
    }
    let (program, args) = opener();
    // Detached, with the streams closed: the file manager outlives the click, and on Linux
    // `xdg-open` chats to stderr about desktop environments it did not find. Inheriting the
    // installer's streams would put that in the terminal a player never opened.
    match Command::new(program)
        .args(args)
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => {
            crate::log_detail(&format!("opened {} in the file manager", folder.display()));
            true
        }
        Err(error) => {
            crate::log_detail(&format!(
                "could not open {} with {program}: {error}",
                folder.display()
            ));
            false
        }
    }
}

/// The command this platform opens a folder with.
///
/// Windows' `explorer` returns a non-zero exit code even when it succeeds, which is one more
/// reason nothing here waits for it. On Linux `xdg-open` is the freedesktop entry point every
/// desktop environment implements; the installer already requires a desktop session, since it
/// draws a window.
const fn opener() -> (&'static str, &'static [&'static str]) {
    if cfg!(windows) {
        ("explorer", &[])
    } else if cfg!(target_os = "macos") {
        ("open", &[])
    } else {
        ("xdg-open", &[])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A folder that is not there is refused before anything is spawned. This is the case a
    /// half-typed path puts the button in, and it must not launch a file manager on a
    /// directory the player has not finished naming.
    #[test]
    fn a_missing_folder_is_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!folder(&dir.path().join("not-there")));
    }

    /// A file is not a folder. `xdg-open` would happily open it in whatever application
    /// claims the extension, which is not what a button labelled with a folder promises.
    #[test]
    fn a_file_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("CvGameCore_Expansion2.dll");
        std::fs::write(&file, "not a folder").unwrap();
        assert!(!folder(&file));
    }

    /// The platform gets the opener its desktop actually provides.
    #[test]
    fn each_platform_has_an_opener() {
        let (program, _) = opener();
        assert!(!program.is_empty());
        if cfg!(windows) {
            assert_eq!(program, "explorer");
        } else if !cfg!(target_os = "macos") {
            assert_eq!(program, "xdg-open");
        }
    }
}
