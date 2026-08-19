//! Running LuaJIT's own build tools.
//!
//! LuaJIT does not ship a prebuilt interpreter core. It generates one, using two programs it
//! compiles first - `minilua`, which runs DynASM, and `buildvm`, which emits the virtual
//! machine as an object file. Both are *host* tools: they run on the machine doing the build.
//!
//! That is a problem for a cross-build, because LuaJIT refuses one unless the host tools have
//! the target's pointer size ("pointer size mismatch in cross-build"). The obvious answer on
//! Linux - a 32-bit host compiler - means multilib, which plenty of installs do not have and
//! which the installer has no way to bootstrap.
//!
//! So the host tools are built as 32-bit *Windows* executables with the same clang the Built
//! DLL uses. The pointer size then matches by construction, on every host, with no compiler
//! the Toolchain Bootstrap does not already provide. On Windows they run directly; on Linux
//! they run under wine.
//!
//! Requiring wine on Linux is not the imposition it looks like: Civilization V has no native
//! build that can load the Built DLL, so every Linux player this installer serves already runs
//! the game through Proton - and Proton ships a wine.

use std::path::{Path, PathBuf};

use crate::build::invoke::ToolCommand;
use crate::error::ToolchainError;

/// How to start a 32-bit Windows executable on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRunner {
    /// Windows: the executable is native, so it is simply run.
    Native,
    /// Linux: run it under this wine binary.
    Wine(PathBuf),
}

impl HostRunner {
    /// Wrap one invocation of a host tool.
    ///
    /// `prefix` is the wine prefix to contain the run in, and is ignored when the host runs
    /// the executable natively.
    pub fn command(
        &self,
        exe: &Path,
        args: Vec<String>,
        current_dir: &Path,
        prefix: &Path,
    ) -> ToolCommand {
        match self {
            Self::Native => ToolCommand::new(exe.to_path_buf(), args, current_dir.to_path_buf()),
            Self::Wine(wine) => {
                let mut all = Vec::with_capacity(args.len() + 1);
                all.push(exe.to_string_lossy().into_owned());
                all.extend(args);
                ToolCommand {
                    program: wine.clone(),
                    args: all,
                    current_dir: current_dir.to_path_buf(),
                    env: vec![
                        // Without this wine builds a prefix in the user's home directory. The
                        // installer's own directory is the only place it may put one.
                        (
                            "WINEPREFIX".to_owned(),
                            prefix.to_string_lossy().into_owned(),
                        ),
                        // Nothing here is .NET or HTML. Left to itself wine notices they are
                        // missing and opens an installer dialog at whoever is running the
                        // build, which for a background compile is simply a bug.
                        ("WINEDLLOVERRIDES".to_owned(), "mscoree,mshtml=".to_owned()),
                        ("WINEDEBUG".to_owned(), "-all".to_owned()),
                    ],
                }
            }
        }
    }

    /// Pick a runner for this host.
    ///
    /// `steam_roots` are the Steam libraries detection already found. Proton lives in one of
    /// them and ships a wine, which is why a player with no system-wide wine still has one -
    /// they must, or the game itself would not run.
    pub fn for_this_host(steam_roots: &[PathBuf]) -> Result<Self, ToolchainError> {
        if cfg!(windows) {
            return Ok(Self::Native);
        }
        proton_wine(steam_roots)
            .or_else(system_wine)
            .map(Self::Wine)
            .ok_or_else(|| {
                ToolchainError::new(
                    "The LuaJIT engine has to be built with wine on Linux, and the installer \
                     could not find one. Install wine, or turn the LuaJIT option off.",
                    format!(
                        "no wine on PATH and none under {} Steam library root(s)",
                        steam_roots.len()
                    ),
                )
            })
    }
}

/// A Proton's wine, if a Steam library holds one.
///
/// Deliberately *any* Proton, not the newest and not the one Steam has mapped to
/// Civilization V. All this wine ever does is run two short console programs that write files
/// and exit; every Proton can do that. Picking the last by name keeps the choice
/// deterministic - note that is lexicographic, so `Proton 9.0` sorts after `Proton 11.0`,
/// which is fine precisely because the version does not matter. Reading Steam's configuration
/// to find the "right" one would be real machinery bought for no benefit.
fn proton_wine(steam_roots: &[PathBuf]) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in steam_roots {
        let Ok(entries) = std::fs::read_dir(root.join("steamapps/common")) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("Proton") {
                continue;
            }
            let wine = entry.path().join("files/bin/wine");
            if wine.is_file() {
                found.push(wine);
            }
        }
    }
    found.sort();
    found.pop()
}

/// A wine on `PATH`.
fn system_wine() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("wine"))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// On Windows the host tools are the target's own architecture and simply run. Wrapping
    /// them in anything would be wrong, and there is no prefix to contain.
    #[test]
    fn a_native_runner_invokes_the_executable_directly() {
        let command = HostRunner::Native.command(
            Path::new("/work/buildvm.exe"),
            vec!["-m".to_owned(), "peobj".to_owned()],
            Path::new("/work"),
            Path::new("/unused"),
        );

        assert_eq!(command.program, PathBuf::from("/work/buildvm.exe"));
        assert_eq!(command.args, vec!["-m".to_owned(), "peobj".to_owned()]);
        assert_eq!(command.current_dir, PathBuf::from("/work"));
        assert!(command.env.is_empty(), "nothing to contain");
    }

    /// Under wine the executable stops being the program and becomes wine's first argument.
    #[test]
    fn a_wine_runner_puts_the_executable_first_among_the_arguments() {
        let command = HostRunner::Wine(PathBuf::from("/usr/bin/wine")).command(
            Path::new("/work/buildvm.exe"),
            vec!["-m".to_owned(), "peobj".to_owned()],
            Path::new("/work"),
            Path::new("/cache/wineprefix"),
        );

        assert_eq!(command.program, PathBuf::from("/usr/bin/wine"));
        assert_eq!(
            command.args,
            vec![
                "/work/buildvm.exe".to_owned(),
                "-m".to_owned(),
                "peobj".to_owned(),
            ]
        );
    }

    /// The containment is the whole reason `env` exists: a build that pops a Mono installer
    /// dialog at the user, or writes a prefix into their home directory, is a broken build.
    #[test]
    fn a_wine_runner_contains_its_prefix_and_silences_the_mono_prompt() {
        let command = HostRunner::Wine(PathBuf::from("/usr/bin/wine")).command(
            Path::new("/work/minilua.exe"),
            Vec::new(),
            Path::new("/work"),
            Path::new("/cache/wineprefix"),
        );

        let env: std::collections::HashMap<&str, &str> = command
            .env
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        assert_eq!(env.get("WINEPREFIX"), Some(&"/cache/wineprefix"));
        assert_eq!(env.get("WINEDLLOVERRIDES"), Some(&"mscoree,mshtml="));
    }

    /// A Steam library with no Proton in it must not be mistaken for one that has it.
    #[test]
    fn an_empty_steam_library_yields_no_proton_wine() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("steamapps/common")).unwrap();
        assert_eq!(proton_wine(&[dir.path().to_path_buf()]), None);
    }

    /// A machine with several Protons picks one and always the same one. Which one is not a
    /// property worth having: any Proton can run a console tool.
    #[test]
    fn one_proton_wine_is_chosen_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        for version in ["Proton 9.0", "Proton 11.0", "Proton 10.0"] {
            let bin = dir
                .path()
                .join("steamapps/common")
                .join(version)
                .join("files/bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("wine"), b"#!/bin/sh\n").unwrap();
        }

        let found = proton_wine(&[dir.path().to_path_buf()]).expect("a Proton wine");
        assert_eq!(
            found,
            proton_wine(&[dir.path().to_path_buf()]).expect("a Proton wine"),
            "the same library must always yield the same wine"
        );
        assert!(found.ends_with("files/bin/wine"), "{found:?}");
    }
}
