//! The real LuaJIT build, against a real LuaJIT checkout and the real Toolchain.
//!
//! `#[ignore]`d, never deleted: the fast suite proves the invocation plan against no compiler
//! at all, and only this proves that the plan, the bootstrapped clang-cl, the extracted SDK
//! and — on Linux — wine together produce a `lua51_Win32.dll` the game can actually load.
//!
//! ```bash
//! CIV5VP_TOOLCHAIN_CACHE=~/.local/share/civ5vp-installer/toolchain-cache \
//! CIV5VP_LUAJIT_SOURCE=/path/to/LuaJIT \
//! CIV5_GAME_DIR="/path/to/Sid Meier's Civilization V" \
//!   cargo test --release -p civ5vp-toolchain --test real_luajit -- --ignored --nocapture
//! ```
//!
//! `CIV5VP_LUAJIT_SOURCE` must be a checkout of the pinned commit — a real `civ5vp-sources`
//! run leaves one at `<App Data Store>/luajit-cache/LuaJIT`. `CIV5_GAME_DIR` is needed because
//! the build refuses to hand back an engine it has not checked against the game's own imports,
//! which is the property this test exists to demonstrate.

use std::path::PathBuf;
use std::sync::mpsc;

use civ5vp_core::{LuaJitBuildRequest, ProgressReporter, ToolchainRunner};
use civ5vp_toolchain::BootstrappedToolchain;

fn from_env(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let path = PathBuf::from(value);
    if !path.exists() {
        println!("{name} points at {} which does not exist", path.display());
        return None;
    }
    Some(path)
}

/// The whole engine build, end to end, checked the way the game will check it: every symbol
/// the game imports from `lua51_Win32.dll` must be exported by what we produced.
#[test]
#[ignore = "compiles LuaJIT; needs a Toolchain Cache, a LuaJIT checkout and a real game"]
fn luajit_builds_into_an_engine_the_game_can_load() {
    let (Some(cache), Some(source), Some(game)) = (
        from_env("CIV5VP_TOOLCHAIN_CACHE"),
        from_env("CIV5VP_LUAJIT_SOURCE"),
        from_env("CIV5_GAME_DIR"),
    ) else {
        println!("set CIV5VP_TOOLCHAIN_CACHE, CIV5VP_LUAJIT_SOURCE and CIV5_GAME_DIR to run");
        return;
    };

    let output_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lua51_Win32.dll");
    let _ = std::fs::remove_file(&output_path);

    let runner = BootstrappedToolchain::new(cache);
    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let printer = std::thread::spawn(move || {
        for event in receiver {
            println!("[{:?}] {}", event.stage, event.message);
        }
    });
    let progress = ProgressReporter::to_channel(sender);

    let started = std::time::Instant::now();
    let outcome = runner.build_luajit(
        &LuaJitBuildRequest {
            source_root: source,
            game_root: game,
            output_path: output_path.clone(),
        },
        &progress,
    );
    drop(progress);
    let _ = printer.join();

    if let Err(problem) = outcome {
        panic!("{}\n  detail: {}", problem.message(), problem.detail());
    }
    println!("LuaJIT built in {:?}", started.elapsed());

    let engine = std::fs::read(&output_path).expect("the engine was written");
    assert_eq!(&engine[..2], b"MZ", "not a PE image");

    // 0x14c is IMAGE_FILE_MACHINE_I386. A 64-bit engine would not load into the game at all,
    // and the failure a player would see is the game simply refusing to start.
    let e_lfanew = u32::from_le_bytes(engine[0x3c..0x40].try_into().expect("4 bytes")) as usize;
    let machine = u16::from_le_bytes(
        engine[e_lfanew + 4..e_lfanew + 6]
            .try_into()
            .expect("2 bytes"),
    );
    assert_eq!(machine, 0x14c, "the engine must be 32-bit x86");

    // The build refuses to produce an engine that fails this, so reaching here already proves
    // it — asserting anyway keeps the proof in the test rather than only in the code.
    let names = String::from_utf8_lossy(&engine);
    assert!(
        names.contains("LuaJIT 2.1"),
        "the engine should report itself as LuaJIT"
    );
    assert!(
        engine.len() > 200_000,
        "suspiciously small engine: {} bytes",
        engine.len()
    );
}
