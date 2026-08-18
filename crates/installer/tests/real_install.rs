//! A real Deployment end to end: real source provider, real toolchain, real compile, real
//! Sync — into throwaway game folders, never the developer's game.
//!
//! `#[ignore]`d. Proves that a real Version installs end to end with a genuinely built
//! DLL, exercised through the same `wiring::core_at` composition
//! the shipped binary uses — only the App Data Store root and the game folders are
//! test-owned.
//!
//! ```bash
//! CIV5VP_TOOLCHAIN_CACHE=~/.cache/civ5vp-toolchain \
//! CIV5VP_DLL_SOURCE_ROOT=/path/to/Community-Patch-DLL \
//!   cargo test --release -p civ5vp-installer --test real_install -- --ignored --nocapture --test-threads 1
//! ```
//!
//! The Installation Source is a Local Repo pointing at `CIV5VP_DLL_SOURCE_ROOT`, so nothing
//! is fetched from the network; the compile is the real one, reusing the Toolchain Cache and
//! any object cache under the store root (`CARGO_TARGET_TMPDIR/real-install-store`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use civ5vp_core::{
    BuildConfiguration, Flavor, FortyThreeCivs, GameFolders, InstallConfiguration, InstallMode,
    InstallationSource, LuaJitEngine, ProgressReporter,
};
use civ5vp_installer::wiring;

/// Point this store's Toolchain Cache at a prepared one, when `CIV5VP_TOOLCHAIN_CACHE` names
/// it, so a real run does not fetch 2.4 GB from archive.org again — that download path has its
/// own proof in `real_bootstrap.rs`.
///
/// Unix only, and quietly nothing elsewhere. The share is a symlink, and creating one on
/// Windows needs a privilege the installer deliberately does not require — the same reason the
/// SDK fix-ups do not run there. On Windows these tests use whatever the store already holds.
fn link_toolchain_cache(store_root: &Path) {
    #[cfg(not(unix))]
    let _ = store_root;
    #[cfg(unix)]
    if let Some(cache) = std::env::var_os("CIV5VP_TOOLCHAIN_CACHE") {
        // `wiring::core_at` puts the Toolchain Cache inside the store root; share the
        // already-populated one instead so these tests never re-download.
        fs::create_dir_all(store_root).unwrap();
        let link = store_root.join("toolchain-cache");
        if !link.exists() {
            std::os::unix::fs::symlink(PathBuf::from(&cache), &link).unwrap();
        }
    }
}

/// The fresh-machine walkthrough: a clean App Data Store, empty game folders, and
/// the exact path a new player takes — list the versions, pick the newest Release, fetch it
/// from the real GitHub, build the DLL, Sync into the game.
///
/// The one concession: `CIV5VP_TOOLCHAIN_CACHE`, when set, is symlinked in as the Toolchain
/// Cache so the run does not re-download 2.4 GB from archive.org every time — that download
/// path has its own proof (`real_bootstrap.rs`). Everything else starts from
/// nothing, including the ~600 MB upstream fetch.
#[test]
#[ignore = "fetches ~600 MB from GitHub and compiles the real DLL; slow"]
fn a_fresh_machine_installs_the_newest_release_from_github() {
    let store_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fresh-machine-store");
    let _ = fs::remove_dir_all(&store_root);
    fs::create_dir_all(&store_root).unwrap();
    link_toolchain_cache(&store_root);
    let game_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fresh-machine-game");
    let _ = fs::remove_dir_all(&game_root);
    let folders = GameFolders {
        mods: game_root.join("Documents/MODS"),
        dlc: game_root.join("Game/Assets/DLC"),
        text: game_root.join("Documents/Text"),
        game_root: game_root.join("Game"),
    };
    for dir in [&folders.mods, &folders.dlc, &folders.text] {
        fs::create_dir_all(dir).unwrap();
    }

    let core = wiring::core_at(&store_root);
    let (progress, printer) = printing_progress();

    // What the picker does at launch: list, take the newest Release.
    let newest = core
        .available_versions(&progress)
        .unwrap_or_else(|error| {
            panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
        })
        .newest_release()
        .expect("upstream always has releases");
    println!("newest release: {newest:?}");

    let configuration = InstallConfiguration {
        source: InstallationSource::UpstreamCache { version: newest },
        // The suggested default is Vox Populi with EUI — the full first-run experience.
        flavor: Flavor::suggested(),
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
    };
    let plan = core.plan(&configuration, &folders).unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });
    let started = std::time::Instant::now();
    let outcome = core.execute(&plan, &progress).unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });
    drop(progress);
    let _ = printer.join();
    println!("fresh install finished in {:?}", started.elapsed());

    let dll = fs::read(&outcome.built_dll).unwrap();
    assert_eq!(&dll[..2], b"MZ");
    assert!(dll.len() > 5_000_000);
    for expected in [
        "(1) Community Patch",
        "(2) Vox Populi",
        "(4a) Squads for VP",
    ] {
        assert!(folders.mods.join(expected).is_dir(), "{expected} missing");
    }
    assert!(folders.dlc.join("VPUI").is_dir());
    assert!(folders.dlc.join("UI_bc1").is_dir());
    assert!(folders.text.join("VPUI_tips_en_us.xml").is_file());
}

fn printing_progress() -> (ProgressReporter, std::thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let printer = std::thread::spawn(move || {
        for event in receiver {
            println!("[{:?}] {}", event.stage, event.message);
        }
    });
    (ProgressReporter::to_channel(sender), printer)
}

#[test]
#[ignore = "needs CIV5VP_DLL_SOURCE_ROOT and a populated Toolchain Cache; compiles the real DLL"]
fn a_real_version_installs_end_to_end_with_a_genuinely_built_dll() {
    let Some(sources) = std::env::var_os("CIV5VP_DLL_SOURCE_ROOT") else {
        panic!("set CIV5VP_DLL_SOURCE_ROOT to a Community-Patch-DLL checkout");
    };
    // The store persists across runs on purpose: the object cache inside it is what makes a
    // re-run of this test cheap, and persisting it is exactly how the shipped installer uses
    // its App Data Store.
    let store_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-install-store");
    link_toolchain_cache(&store_root);

    // Throwaway game folders with the real layout: MODS and Text as siblings in a Documents
    // folder, DLC under the game's Assets.
    let game_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-install-game");
    let _ = fs::remove_dir_all(&game_root);
    let folders = GameFolders {
        mods: game_root.join("Documents/MODS"),
        dlc: game_root.join("Game/Assets/DLC"),
        text: game_root.join("Documents/Text"),
        game_root: game_root.join("Game"),
    };
    for dir in [&folders.mods, &folders.dlc, &folders.text] {
        fs::create_dir_all(dir).unwrap();
    }

    let core = wiring::core_at(&store_root);
    let configuration = InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: PathBuf::from(sources),
        },
        flavor: Flavor::CommunityPatch,
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
    };
    let plan = core.plan(&configuration, &folders).unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });

    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let printer = std::thread::spawn(move || {
        for event in receiver {
            println!("[{:?}] {}", event.stage, event.message);
        }
    });
    let progress = ProgressReporter::to_channel(sender);
    let outcome = core.execute(&plan, &progress).unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });
    drop(progress);
    let _ = printer.join();

    // The Deployment placed the Built DLL at the root of (1) Community Patch, and it is a
    // real PE DLL, not a marker.
    let deployed = folders
        .mods
        .join("(1) Community Patch/CvGameCore_Expansion2.dll");
    assert_eq!(outcome.built_dll, deployed);
    let bytes = fs::read(&deployed).unwrap();
    assert_eq!(&bytes[..2], b"MZ", "deployed DLL must be a PE binary");
    assert!(
        bytes.len() > 5_000_000,
        "a genuinely compiled DLL is megabytes, got {} bytes",
        bytes.len()
    );
    // And the mod content came along with it.
    assert!(
        folders
            .mods
            .join("(1) Community Patch/(1) Community Patch.modinfo")
            .is_file()
            || fs::read_dir(folders.mods.join("(1) Community Patch"))
                .unwrap()
                .count()
                > 1,
        "the Community Patch folder must hold more than the DLL"
    );
    println!("installed {} bytes to {}", bytes.len(), deployed.display());

    // Repeat install: same configuration, nothing changed — the Build
    // Fingerprint sidecar and the intact DLL make the second run skip the build entirely.
    let started = std::time::Instant::now();
    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let progress = ProgressReporter::to_channel(sender);
    let plan = core.plan(&configuration, &folders).unwrap();
    core.execute(&plan, &progress).unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });
    drop(progress);
    let lines: Vec<String> = receiver.iter().map(|event| event.message).collect();
    assert!(
        lines.iter().any(|line| line.contains("already up to date")),
        "the repeat install must skip the build: {lines:?}"
    );
    let again = fs::read(&deployed).unwrap();
    assert_eq!(
        again.len(),
        bytes.len(),
        "the deployed DLL survives the skip"
    );
    println!(
        "repeat install skipped the build in {:?}",
        started.elapsed()
    );

    // The edit-to-game loop: edit a Lua file in the checkout, redeploy, and the
    // change is in MODS — without the DLL recompiling. The fixture checkout is shared with
    // other tests, so the file is restored afterwards (a panic in between leaves an edit
    // behind; re-materializing the Version puts it right).
    let lua = PathBuf::from(std::env::var_os("CIV5VP_DLL_SOURCE_ROOT").unwrap())
        .join("(1) Community Patch/LUA/AssignStartingPlots.lua");
    let original = fs::read(&lua).unwrap();
    let mut edited = original.clone();
    edited.extend_from_slice(b"\n-- hot-reload demo edit\n");
    fs::write(&lua, &edited).unwrap();

    let started = std::time::Instant::now();
    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let progress = ProgressReporter::to_channel(sender);
    let plan = core.plan(&configuration, &folders).unwrap();
    let redeploy = core.execute(&plan, &progress);
    drop(progress);
    let lines: Vec<String> = receiver.iter().map(|event| event.message).collect();
    fs::write(&lua, &original).unwrap();
    redeploy.unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });

    assert!(
        lines.iter().any(|line| line.contains("already up to date")),
        "a Lua edit must not recompile the DLL: {lines:?}"
    );
    let deployed_lua = fs::read(
        folders
            .mods
            .join("(1) Community Patch/LUA/AssignStartingPlots.lua"),
    )
    .unwrap();
    assert!(
        deployed_lua.ends_with(b"\n-- hot-reload demo edit\n"),
        "the edited Lua must be what reached MODS"
    );
    println!(
        "edited Lua redeployed without a rebuild in {:?}",
        started.elapsed()
    );
}

/// The Replaced File, end to end through the real Core: fetch LuaJIT, build it with the real
/// toolchain, replace the engine, then give the original back.
///
/// The game folders are throwaway, but the Game Installation they point at holds real copies
/// of Civilization V's own binaries — the build refuses to hand back an engine it has not
/// checked against those, so a fixture of empty files would not exercise the thing worth
/// proving. Nothing under `CIV5_GAME_DIR` is written; it is only read from.
///
/// ```bash
/// CIV5VP_TOOLCHAIN_CACHE=~/.local/share/civ5vp-installer/toolchain-cache \
/// CIV5VP_DLL_SOURCE_ROOT=/path/to/Community-Patch-DLL \
/// CIV5_GAME_DIR="/path/to/Sid Meier's Civilization V" \
///   cargo test --release -p civ5vp-installer --test real_install -- --ignored --nocapture \
///   luajit_is_built_deployed_and_restored
/// ```
#[test]
#[ignore = "fetches LuaJIT and compiles it and the DLL; needs a real Civilization V"]
fn luajit_is_built_deployed_and_restored() {
    let (Some(sources), Some(real_game)) = (
        std::env::var_os("CIV5VP_DLL_SOURCE_ROOT").map(PathBuf::from),
        std::env::var_os("CIV5_GAME_DIR").map(PathBuf::from),
    ) else {
        println!("set CIV5VP_DLL_SOURCE_ROOT and CIV5_GAME_DIR to run");
        return;
    };

    let store_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("luajit-store");
    link_toolchain_cache(&store_root);

    let game_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("luajit-game");
    let _ = fs::remove_dir_all(&game_root);
    let folders = GameFolders {
        mods: game_root.join("Documents/MODS"),
        dlc: game_root.join("Game/Assets/DLC"),
        text: game_root.join("Documents/Text"),
        game_root: game_root.join("Game"),
    };
    for dir in [&folders.mods, &folders.dlc, &folders.text] {
        fs::create_dir_all(dir).unwrap();
    }

    // The binaries the engine is checked against, plus the engine it will replace.
    for name in [
        "CivilizationV_DX11.exe",
        "CvGameCoreDLLFinal Release.dll",
        "CvGameDatabaseWin32Final Release.dll",
        "lua51_Win32.dll",
    ] {
        fs::copy(real_game.join(name), folders.game_root.join(name))
            .unwrap_or_else(|error| panic!("copying {name} out of the real game: {error}"));
    }
    let engine = folders.game_root.join("lua51_Win32.dll");
    let stock = fs::read(&engine).unwrap();
    assert!(
        String::from_utf8_lossy(&stock).contains("Lua 5.1"),
        "this test has to start from the stock engine"
    );

    let core = wiring::core_at(&store_root);
    let configuration = InstallConfiguration {
        source: InstallationSource::LocalRepo { path: sources },
        flavor: Flavor::CommunityPatch,
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::LuaJit,
    };

    let plan = core.plan(&configuration, &folders).unwrap_or_else(|error| {
        panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
    });
    let started = std::time::Instant::now();
    let outcome = core
        .execute(&plan, &ProgressReporter::silent())
        .unwrap_or_else(|error| {
            panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
        });
    println!("LuaJIT deployed in {:?}", started.elapsed());

    assert_eq!(outcome.engine, civ5vp_core::EngineOutcome::Replaced);
    let replaced = fs::read(&engine).unwrap();
    assert!(
        String::from_utf8_lossy(&replaced).contains("LuaJIT"),
        "the engine in the game should now be LuaJIT"
    );

    let uninstalled = core
        .uninstall(&folders, &ProgressReporter::silent())
        .unwrap_or_else(|error| {
            panic!("{}\n  detail: {}", error.user_message(), error.log_detail())
        });
    assert_eq!(
        uninstalled.engine_restored,
        civ5vp_core::Restored::FromBackup
    );
    assert_eq!(
        fs::read(&engine).unwrap(),
        stock,
        "Uninstall must give the player their engine back byte for byte"
    );
}
