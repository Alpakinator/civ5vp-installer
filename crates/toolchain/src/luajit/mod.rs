//! Building LuaJIT into the game's Lua engine.
//!
//! The one failure mode that would silently brick a player's game is a `lua51_Win32.dll` that
//! is missing a symbol the game imports from it, so [`exports`] checks for exactly that
//! before any build is allowed near the Game Installation.

pub mod build;
pub mod exports;
pub mod host;
pub mod patches;

use std::path::{Path, PathBuf};

use civ5vp_core::{LuaJitBuildRequest, ProgressReporter, Stage};

use crate::build::invoke::ToolInvoker;
use crate::cache::Toolchain;
use crate::error::ToolchainError;
use build::{ENGINE_FILE_NAME, LUAJIT_RELVER, LuaJitBuild};
use host::HostRunner;

/// The game binaries whose Lua imports the built engine has to satisfy.
///
/// The two stock DLLs and the executable are always there. Vox Populi's own gamecore DLL is
/// not — it lives in the MODS Folder and is checked separately when it can be found — so this
/// is the set that exists in any Game Installation.
const GAME_CONSUMERS: [&str; 3] = [
    "CivilizationV_DX11.exe",
    "CvGameCoreDLLFinal Release.dll",
    "CvGameDatabaseWin32Final Release.dll",
];

/// Build LuaJIT and leave it at `request.output_path`.
pub fn run(
    toolchain: &Toolchain,
    invoker: &dyn ToolInvoker,
    steam_roots: &[PathBuf],
    request: &LuaJitBuildRequest,
    progress: &ProgressReporter,
) -> Result<(), ToolchainError> {
    let src = request.source_root.join("src");
    if !src.is_dir() {
        return Err(ToolchainError::new(
            "The LuaJIT source the installer downloaded is not complete.",
            format!("no src directory under {}", request.source_root.display()),
        ));
    }

    let host = HostRunner::for_this_host(steam_roots)?;
    let wine_prefix = toolchain.llvm_root().join("../luajit-wineprefix");
    let plan = LuaJitBuild {
        clang: toolchain.clang_path(),
        lld_link: toolchain.lld_link_path(),
        include_dirs: toolchain.include_dirs()?,
        lib_dirs: toolchain.lib_dirs()?,
        host,
        wine_prefix: wine_prefix.clone(),
    };

    // Where LuaJIT and the engine it replaces disagree about behaviour Lua leaves undefined,
    // the game's mods were written against the other answer. This is where that is closed.
    patches::apply(&src)?;

    // `genversion.lua` reads this rather than being told; upstream's own script writes it the
    // same way, from `git` where we use the pinned constant.
    std::fs::write(src.join("luajit_relver.txt"), LUAJIT_RELVER)
        .map_err(|error| crate::error::io_error("write the LuaJIT version stamp", &src, &error))?;
    if matches!(plan.host, HostRunner::Wine(_)) {
        std::fs::create_dir_all(&wine_prefix).map_err(|error| {
            crate::error::io_error("create the wine prefix", &wine_prefix, &error)
        })?;
    }

    progress.report(Stage::Build, "Building the LuaJIT engine.");
    let sources = build::library_sources(&src)?;
    for command in plan.commands(&src, &sources) {
        let output = invoker.run(&command).map_err(|problem| {
            ToolchainError::new("The LuaJIT engine could not be built.", problem)
        })?;
        if !output.success {
            return Err(ToolchainError::new(
                "The LuaJIT engine could not be built, so your game was not changed.",
                format!(
                    "{} {}\n{}",
                    command.program.display(),
                    command.args.join(" "),
                    output.output
                ),
            ));
        }
    }

    let built = src.join(ENGINE_FILE_NAME);
    check_the_game_can_use_it(&built, &request.game_root)?;

    if let Some(parent) = request.output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| crate::error::io_error("create the build folder", parent, &error))?;
    }
    std::fs::copy(&built, &request.output_path).map_err(|error| {
        crate::error::io_error("copy the built engine", &request.output_path, &error)
    })?;
    Ok(())
}

/// Refuse an engine the game could not load.
///
/// A `lua51_Win32.dll` missing even one imported symbol leaves the player unable to start
/// Civilization V at all, and unlike most build faults it is entirely checkable beforehand. So
/// it is checked, on every build, before the file goes anywhere near the Game Installation.
fn check_the_game_can_use_it(built: &Path, game_root: &Path) -> Result<(), ToolchainError> {
    let engine = std::fs::read(built).map_err(|error| {
        crate::error::io_error("read the engine that was just built", built, &error)
    })?;

    let mut consumers: Vec<Vec<u8>> = Vec::new();
    for name in GAME_CONSUMERS {
        let path = game_root.join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            // A Game Installation missing one of these is not this build's problem to
            // diagnose; the Core rejected it long before, and checking against whatever *is*
            // present is strictly better than refusing to check at all.
            continue;
        };
        consumers.push(bytes);
    }
    if consumers.is_empty() {
        return Err(ToolchainError::new(
            "The installer could not check the LuaJIT engine against your game, so it did not \
             install it.",
            format!(
                "none of {GAME_CONSUMERS:?} could be read from {}",
                game_root.display()
            ),
        ));
    }

    let borrowed: Vec<&[u8]> = consumers.iter().map(Vec::as_slice).collect();
    let Some(missing) = exports::missing_for(&engine, &borrowed) else {
        return Err(ToolchainError::new(
            "The installer could not check the LuaJIT engine against your game, so it did not \
             install it.",
            "the engine or one of the game's binaries did not parse as a Windows executable"
                .to_owned(),
        ));
    };
    if !missing.is_empty() {
        return Err(ToolchainError::new(
            "The LuaJIT engine the installer built is missing functions your game needs, so it \
             was not installed. Your game was not changed.",
            format!("missing exports: {}", missing.join(", ")),
        ));
    }
    Ok(())
}
