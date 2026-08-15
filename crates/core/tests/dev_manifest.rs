//! Dev mode holds a checkout against each mod's own `.modinfo` manifest.
//!
//! The scenario is a mod developer's everyday drift: files added to the working tree that
//! the modinfo does not list yet (the game will silently ignore them), and files deleted
//! while the modinfo still lists them (the game would refuse the mod). The first must be
//! said out loud, the second must stop the Deployment before the game is touched — and an
//! Upstream Cache Version must trigger neither, because a player cannot act on upstream's
//! manifest hygiene.

mod support;

use std::fs;
use std::path::Path;

use civ5vp_core::{
    BuildConfiguration, Core, Eui, Flavor, FortyThreeCivs, InstallConfiguration, InstallError,
    InstallMode, InstallationSource, ProgressReporter, Version,
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
        "the file still deploys — blocking it would break normal dev iteration"
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

/// The breaking case: a listed file is gone, so nothing is deployed and the sentence names
/// the file and the fix.
#[test]
fn a_missing_listed_file_stops_the_deployment() {
    let game = GameFixture::new();
    let checkout = editable_repo(&game);
    fs::remove_file(checkout.join("(2) Vox Populi/LUA/PlotHelpManager.lua")).unwrap();
    let before = game.files();
    let core = core_over(&game, &checkout);

    let plan = core.plan(&local_repo(&checkout), &game.folders()).unwrap();
    let error = core
        .execute(&plan, &ProgressReporter::silent())
        .expect_err("a mod the game would refuse must not be deployed");

    assert!(matches!(error, InstallError::ModManifestMismatch { .. }));
    let message = error.user_message();
    assert!(message.contains("PlotHelpManager.lua"), "got: {message}");
    assert!(message.contains("(2) Vox Populi"), "got: {message}");
    assert_eq!(game.files(), before, "rule 7: the game is untouched");
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
