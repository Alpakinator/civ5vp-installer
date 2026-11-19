//! Dev mode holds a checkout against each mod's own `.modinfo` manifest.
//!
//! The scenario is a mod developer's everyday drift: files added to the working tree that
//! the modinfo does not list yet (the game will silently ignore them), and files deleted
//! while the modinfo still lists them (the game skips them and loads the rest, as every EUI
//! install shows - ADR-0013). Both must be said out loud and neither stops the Deployment -
//! and an Upstream Cache Version must trigger neither, because a player cannot act on
//! upstream's manifest hygiene.

mod support;

use std::fs;
use std::path::Path;

use civ5vp_core::{
    BuildConfiguration, Core, DllSource, Eui, Flavor, FortyThreeCivs, InstallConfiguration,
    InstallMode, InstallationSource, LuaJitEngine, ProgressReporter, Version,
};
use support::{
    FixtureModpackAssembler, FixtureSourceProvider, GameFixture, MarkerToolchainRunner,
    miniature_repo,
};

/// A private, editable copy of the miniature repository, inside the fixture's tempdir.
fn editable_repo(game: &GameFixture) -> std::path::PathBuf {
    let copy = game.work_dir().join("checkout");
    copy_tree(&miniature_repo(), &copy);
    copy
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap().path();
        let target = to.join(entry.file_name().unwrap());
        if entry.is_dir() {
            copy_tree(&entry, &target);
        } else {
            fs::copy(&entry, &target).unwrap();
        }
    }
}

fn local_repo(checkout: &Path) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::LocalRepo {
            path: checkout.to_path_buf(),
        },
        flavor: Flavor::VoxPopuli { eui: Eui::Disabled },
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
        menu_theme: civ5vp_core::MenuTheme::Stock,
        opening_movie: civ5vp_core::OpeningMovie::Stock,
        dll_source: DllSource::ShippedWhenCurrent,
    }
}

fn core_over(game: &GameFixture, checkout: &Path) -> Core {
    Core::new(
        Box::new(FixtureSourceProvider::new(checkout.to_path_buf())),
        Box::new(MarkerToolchainRunner),
        Box::new(FixtureModpackAssembler::ignored()),
        game.work_dir(),
    )
}

/// The "why does my change do nothing" case: the file deploys, and the activity log says
/// the game will ignore it.
#[test]
fn an_extra_unlisted_file_is_deployed_but_called_out() {
    let game = GameFixture::new();
    let checkout = editable_repo(&game);
    fs::write(
        checkout.join("(2) Vox Populi/Experimental.lua"),
        "-- not in the modinfo yet",
    )
    .unwrap();
    let core = core_over(&game, &checkout);

    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core.plan(&local_repo(&checkout), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .expect("an unlisted extra file must not stop a Deployment");

    assert!(
        game.game_root()
            .join("MODS/(2) Vox Populi/Experimental.lua")
            .is_file(),
        "the file still deploys - blocking it would break normal dev iteration"
    );
    let warnings: Vec<String> = receiver
        .try_iter()
        .filter(|event| event.message.contains("does not list"))
        .map(|event| event.message)
        .collect();
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("Experimental.lua") && line.contains("(2) Vox Populi")),
        "the activity log should name the unlisted file, got: {warnings:?}"
    );
}

/// A listed file is gone: the game skips the entry and loads the rest of the mod, so the
/// Deployment goes ahead - and the log file keeps a line naming the file and the mod.
#[test]
fn a_missing_listed_file_is_called_out_and_the_deployment_goes_ahead() {
    let game = GameFixture::new();
    let checkout = editable_repo(&game);
    fs::remove_file(checkout.join("(2) Vox Populi/LUA/PlotHelpManager.lua")).unwrap();
    let core = core_over(&game, &checkout);

    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core.plan(&local_repo(&checkout), &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .expect("the game loads a mod whose modinfo lists a file it lacks");

    assert!(
        game.game_root()
            .join("MODS/(2) Vox Populi/Balance Changes/BalanceChanges.sql")
            .is_file(),
        "the rest of the mod is deployed"
    );
    let kept: Vec<String> = receiver
        .try_iter()
        .filter(|event| event.heads_up)
        .map(|event| event.message)
        .collect();
    assert_eq!(
        kept,
        [
            "(2) Vox Populi's .modinfo lists 1 file(s) that are not in the folder, and the game \
          will skip them: LUA\\PlotHelpManager.lua"
        ],
    );
}

/// An Upstream Cache Version ships what upstream released: the same drift neither warns
/// nor stops anything.
#[test]
fn an_upstream_version_is_never_held_against_its_manifest() {
    let game = GameFixture::new();
    let checkout = editable_repo(&game);
    fs::write(
        checkout.join("(2) Vox Populi/Experimental.lua"),
        "-- unlisted",
    )
    .unwrap();
    fs::remove_file(checkout.join("(2) Vox Populi/LUA/PlotHelpManager.lua")).unwrap();
    let core = core_over(&game, &checkout);

    let mut configuration = local_repo(&checkout);
    configuration.source = InstallationSource::UpstreamCache {
        version: Version::Release("Release-2.0".to_owned()),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let plan = core.plan(&configuration, &game.folders()).unwrap();
    core.execute(&plan, &ProgressReporter::to_channel(sender))
        .expect("upstream Versions are deployed as released");

    assert!(
        !receiver
            .try_iter()
            .any(|event| event.message.contains("does not list")),
        "no manifest warnings for an upstream Version"
    );
}
