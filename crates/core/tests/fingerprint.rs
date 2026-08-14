//! The Build Fingerprint, through the Core seam (ticket 07).
//!
//! Everything is observed from outside: the file tree, the progress events, and how many
//! times the toolchain-runner boundary was asked to build — a skipped build is a build the
//! fake runner never saw. The fixture repository is copied into a temp directory first, so a
//! test can edit a source file the way a developer would.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use civ5vp_core::{
    Core, Flavor, FortyThreeCivs, InstallConfiguration, InstallationSource, ProgressReporter,
};
use support::{
    CountingToolchainRunner, DLL_MARKER, FixtureSourceProvider, GameFixture, miniature_repo,
};

/// A private, editable copy of the miniature repository.
fn editable_repo(into: &Path) -> PathBuf {
    let destination = into.join("editable-repo");
    copy_tree(&miniature_repo(), &destination);
    destination
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn configuration(repo: &Path, forty_three_civs: FortyThreeCivs) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: repo.to_path_buf(),
        },
        flavor: Flavor::CommunityPatch,
        forty_three_civs,
    }
}

fn core_over(game: &GameFixture, repo: &Path, identity: &str) -> (Core, Arc<AtomicUsize>) {
    let (runner, builds) = CountingToolchainRunner::new(identity);
    let core = Core::new(
        Box::new(FixtureSourceProvider::new(repo.to_path_buf())),
        Box::new(runner),
        game.work_dir(),
    );
    (core, builds)
}

fn install(core: &Core, game: &GameFixture, configuration: &InstallConfiguration) -> Vec<String> {
    let plan = core.plan(configuration, &game.folders()).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .unwrap();
    receiver
        .iter()
        .map(|event| format!("{:?}: {}", event.stage, event.message))
        .collect()
}

/// The headline behaviour (user story 17): same inputs, intact DLL, no second build — and
/// the skip is said out loud.
#[test]
fn an_unchanged_configuration_skips_the_build_and_says_so() {
    let game = GameFixture::new();
    let repo = editable_repo(game.work_dir().as_path());
    let (core, builds) = core_over(&game, &repo, "fake-toolchain-0");
    let config = configuration(&repo, FortyThreeCivs::Disabled);

    let first = install(&core, &game, &config);
    let second = install(&core, &game, &config);

    assert_eq!(builds.load(Ordering::Relaxed), 1, "one build, one skip");
    assert!(
        second
            .iter()
            .any(|line| line.contains("already up to date")),
        "the skip must be reported: {second:?}"
    );
    assert!(!first.iter().any(|line| line.contains("already up to date")));
    // The Deployment is still complete and exact after a skipped build.
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER
    );
}

/// Editing a DLL source forces a rebuild; editing mod content does not, but still deploys —
/// "no false skips, no needless rebuilds", both halves.
#[test]
fn an_edited_source_rebuilds_but_edited_mod_content_only_redeploys() {
    let game = GameFixture::new();
    let repo = editable_repo(game.work_dir().as_path());
    let (core, builds) = core_over(&game, &repo, "fake-toolchain-0");
    let config = configuration(&repo, FortyThreeCivs::Disabled);
    install(&core, &game, &config);

    // Mod content only: Lua changes, the DLL inputs do not.
    fs::write(
        repo.join("(1) Community Patch/LUA/CityView.lua"),
        "-- hotfixed\n",
    )
    .unwrap();
    install(&core, &game, &config);
    assert_eq!(
        builds.load(Ordering::Relaxed),
        1,
        "a Lua edit must not compile"
    );
    assert_eq!(
        game.read("MODS/(1) Community Patch/LUA/CityView.lua"),
        "-- hotfixed\n",
        "but it must deploy"
    );

    // A DLL source: rebuild.
    fs::write(
        repo.join("CvGameCoreDLL_Expansion2/CvGame.cpp"),
        "// edited\n",
    )
    .unwrap();
    install(&core, &game, &config);
    assert_eq!(
        builds.load(Ordering::Relaxed),
        2,
        "a source edit must rebuild"
    );
}

/// Each remaining invalidation from the ticket, alone, forces a rebuild.
#[test]
fn configuration_toolchain_tamper_and_missing_sidecar_each_force_a_rebuild() {
    let game = GameFixture::new();
    let repo = editable_repo(game.work_dir().as_path());
    let (core, builds) = core_over(&game, &repo, "fake-toolchain-0");
    install(
        &core,
        &game,
        &configuration(&repo, FortyThreeCivs::Disabled),
    );
    assert_eq!(builds.load(Ordering::Relaxed), 1);

    // A different configuration: the 43-Civs toggle.
    install(&core, &game, &configuration(&repo, FortyThreeCivs::Enabled));
    assert_eq!(builds.load(Ordering::Relaxed), 2, "43-Civs invalidates");

    // A different Toolchain version, same everything else.
    let (newer_toolchain, newer_builds) = core_over(&game, &repo, "fake-toolchain-1");
    install(
        &newer_toolchain,
        &game,
        &configuration(&repo, FortyThreeCivs::Enabled),
    );
    assert_eq!(
        newer_builds.load(Ordering::Relaxed),
        1,
        "toolchain invalidates"
    );

    // A tampered deployed DLL: the sidecar still matches, the bytes do not.
    let deployed = game
        .game_root()
        .join("MODS/(1) Community Patch/CvGameCore_Expansion2.dll");
    fs::write(&deployed, "swapped in by hand").unwrap();
    install(
        &newer_toolchain,
        &game,
        &configuration(&repo, FortyThreeCivs::Enabled),
    );
    assert_eq!(
        newer_builds.load(Ordering::Relaxed),
        2,
        "tampering invalidates"
    );
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER
    );

    // A missing sidecar.
    fs::remove_file(
        game.game_root()
            .join("MODS/(1) Community Patch/CvGameCore_Expansion2.dll.fingerprint"),
    )
    .unwrap();
    install(
        &newer_toolchain,
        &game,
        &configuration(&repo, FortyThreeCivs::Enabled),
    );
    assert_eq!(
        newer_builds.load(Ordering::Relaxed),
        3,
        "a missing sidecar invalidates"
    );
}

/// A deleted deployed DLL cannot be "skipped to" however fresh the sidecar looks.
#[test]
fn a_missing_deployed_dll_forces_a_rebuild() {
    let game = GameFixture::new();
    let repo = editable_repo(game.work_dir().as_path());
    let (core, builds) = core_over(&game, &repo, "fake-toolchain-0");
    let config = configuration(&repo, FortyThreeCivs::Disabled);
    install(&core, &game, &config);
    fs::remove_file(
        game.game_root()
            .join("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
    )
    .unwrap();

    install(&core, &game, &config);

    assert_eq!(builds.load(Ordering::Relaxed), 2);
    assert_eq!(
        game.read("MODS/(1) Community Patch/CvGameCore_Expansion2.dll"),
        DLL_MARKER
    );
}
