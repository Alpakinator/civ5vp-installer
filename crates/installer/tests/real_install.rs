//! A real Deployment end to end: real source provider, real toolchain, real compile, real
//! Sync — into throwaway game folders, never the developer's game.
//!
//! `#[ignore]`d (rule 14). This is the ticket-06 acceptance "a real Version installs end to
//! end with a genuinely built DLL", exercised through the same `wiring::core_at` composition
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
use std::path::PathBuf;
use std::sync::mpsc;

use civ5vp_core::{
    BuildConfiguration, Flavor, FortyThreeCivs, GameFolders, InstallConfiguration,
    InstallationSource, ProgressReporter,
};
use civ5vp_installer::wiring;

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
    if let Some(cache) = std::env::var_os("CIV5VP_TOOLCHAIN_CACHE") {
        // `wiring::core_at` puts the Toolchain Cache inside the store root; share the
        // already-populated one instead through a symlink so this test never re-downloads.
        fs::create_dir_all(&store_root).unwrap();
        let link = store_root.join("toolchain-cache");
        if !link.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(PathBuf::from(&cache), &link).unwrap();
        }
    }

    // Throwaway game folders with the real layout: MODS and Text as siblings in a Documents
    // folder, DLC under the game's Assets.
    let game_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-install-game");
    let _ = fs::remove_dir_all(&game_root);
    let folders = GameFolders {
        mods: game_root.join("Documents/MODS"),
        dlc: game_root.join("Game/Assets/DLC"),
        text: game_root.join("Documents/Text"),
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

    // Repeat install (user story 17): same configuration, nothing changed — the Build
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

    // Ticket 08's edit-to-game loop: edit a Lua file in the checkout, redeploy, and the
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
