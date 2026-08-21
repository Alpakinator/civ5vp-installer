//! The Shipped DLL, through the Core seam.
//!
//! Upstream refreshes `CvGameCore_Expansion2.dll` in the Release commit itself and nowhere
//! else, so at a Release - and only there - the DLL checked in beside the sources was built
//! from them. These tests are about the one decision that follows: deploy that file, or
//! compile one.
//!
//! Observed from outside, house style. Whether a build happened is not asked of the Core; it
//! is read off the toolchain-runner boundary, which either counted a call or never saw one.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use civ5vp_core::{
    BuildConfiguration, Core, DllSource, Flavor, FortyThreeCivs, InstallConfiguration, InstallMode,
    InstallationSource, LuaJitEngine, ProgressReporter, Version,
};
use support::{
    CountingToolchainRunner, DLL_MARKER, FixtureModpackAssembler, FixtureSourceProvider,
    GameFixture, miniature_repo,
};

/// Where the deployed DLL and the two the fixture ships live.
const DEPLOYED: &str = "MODS/(1) Community Patch/CvGameCore_Expansion2.dll";
const SHIPPED: &str = "(1) Community Patch/CvGameCore_Expansion2.dll";
const SHIPPED_43: &str = "(3b) 43 Civs Community Patch/CvGameCore_Expansion2.dll";

/// An install of `Release-2.0` from the Upstream Cache - the default path a player takes.
fn a_release(forty_three_civs: FortyThreeCivs, dll_source: DllSource) -> InstallConfiguration {
    InstallConfiguration {
        source: InstallationSource::UpstreamCache {
            version: Version::Release("Release-2.0".to_owned()),
        },
        flavor: Flavor::CommunityPatch,
        forty_three_civs,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
        luajit: LuaJitEngine::Stock,
        dll_source,
    }
}

/// A Core whose source provider stands in for a Release commit: the checked-in DLLs in
/// `repo` are the ones that Version was released with.
fn core_at_a_release(
    game: &GameFixture,
    repo: PathBuf,
) -> (Core, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let (runner, builds) = CountingToolchainRunner::new("fake-toolchain-0");
    let core = Core::new(
        Box::new(FixtureSourceProvider::at_a_release_commit(repo)),
        Box::new(runner),
        Box::new(FixtureModpackAssembler::ignored()),
        game.work_dir(),
    );
    (core, builds)
}

/// A Core whose source provider vouches for nothing - every other Version, and every Local
/// Repo.
fn core_elsewhere(
    game: &GameFixture,
    repo: PathBuf,
) -> (Core, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let (runner, builds) = CountingToolchainRunner::new("fake-toolchain-0");
    let core = Core::new(
        Box::new(FixtureSourceProvider::new(repo)),
        Box::new(runner),
        Box::new(FixtureModpackAssembler::ignored()),
        game.work_dir(),
    );
    (core, builds)
}

/// A private copy of the miniature repository, so a test can delete a file out of it.
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

fn shipped(repo: &Path, relative: &str) -> String {
    fs::read_to_string(repo.join(relative)).unwrap()
}

/// The headline: a Release install deploys the DLL that Release ships and compiles nothing.
///
/// The build count is the whole point. A Deployment that produced the right file by
/// compiling it would pass a file-tree assertion and still cost the player the Toolchain
/// Bootstrap's multi-gigabyte download - which is the thing this is here to prevent.
#[test]
fn a_release_deploys_the_dll_it_ships_and_never_builds() {
    let game = GameFixture::new();
    let (core, builds) = core_at_a_release(&game, miniature_repo());

    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent())
        .expect("a release install should succeed");

    assert_eq!(builds.load(Ordering::Relaxed), 0, "nothing should be built");
    assert_eq!(game.read(DEPLOYED), shipped(&miniature_repo(), SHIPPED));
}

/// With 43 Civs on, the Shipped DLL comes out of `(3b)` and still lands in `(1)`.
///
/// `(3b)` itself deploys as it always has - its modinfo and `AdvancedSetup.lua`, no DLL -
/// because `(1)` is where the modinfo's hook looks for one. Taking the DLL from `(3b)` and
/// deploying it into `(1)` is the same move the compile path makes with the 43-civ build.
#[test]
fn a_forty_three_civ_release_deploys_the_dll_from_3b() {
    let game = GameFixture::new();
    let (core, builds) = core_at_a_release(&game, miniature_repo());

    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Enabled, DllSource::ShippedWhenCurrent),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(builds.load(Ordering::Relaxed), 0);
    assert_eq!(game.read(DEPLOYED), shipped(&miniature_repo(), SHIPPED_43));
    assert!(
        !game
            .game_root()
            .join("MODS/(3b) 43 Civs Community Patch/CvGameCore_Expansion2.dll")
            .exists(),
        "(3b) still deploys without a DLL of its own"
    );
}

/// Anything that is not a Release commit is built, however new its checked-in DLL looks.
///
/// This is ADR-0001 unchanged for every Version but one: an unofficial build, a branch, a
/// commit typed in by hand, or a Local Repo all carry a DLL older than the sources beside
/// it, and none of them may be trusted.
#[test]
fn a_version_that_is_not_a_release_is_built() {
    let game = GameFixture::new();
    let (core, builds) = core_elsewhere(&game, miniature_repo());

    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(builds.load(Ordering::Relaxed), 1);
    assert_eq!(game.read(DEPLOYED), DLL_MARKER);
}

/// A Release that ships no DLL for this configuration is built rather than refused.
///
/// Old tags predate one or both of the checked-in DLLs. Refusing would leave a player with a
/// Version the picker offered and the installer would not install; building costs them the
/// toolchain download and gets them the mod.
#[test]
fn a_release_with_no_shipped_dll_is_built_instead() {
    let game = GameFixture::new();
    let repo = editable_repo(game.work_dir().parent().unwrap());
    fs::remove_file(repo.join(SHIPPED)).unwrap();
    let (core, builds) = core_at_a_release(&game, repo);

    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent())
        .expect("a release missing its DLL should still install");

    assert_eq!(builds.load(Ordering::Relaxed), 1);
    assert_eq!(game.read(DEPLOYED), DLL_MARKER);
}

/// "Compile the DLL myself" is honoured even where a current Shipped DLL is right there.
#[test]
fn asking_to_compile_overrides_a_current_shipped_dll() {
    let game = GameFixture::new();
    let (core, builds) = core_at_a_release(&game, miniature_repo());

    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::AlwaysCompile),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();

    assert_eq!(builds.load(Ordering::Relaxed), 1);
    assert_eq!(game.read(DEPLOYED), DLL_MARKER);
}

/// Ticking the box after a Shipped-DLL install really rebuilds, and unticking it goes back.
///
/// The Build Fingerprint sidecar is what makes the second install cheap, and every input to
/// it is the same across these two runs except where the DLL came from. Without that in the
/// fingerprint the installer would find a match, skip, and leave the player looking at the
/// file they just asked it to replace.
#[test]
fn switching_between_shipped_and_compiled_is_not_mistaken_for_no_change() {
    let game = GameFixture::new();

    let (core, builds) = core_at_a_release(&game, miniature_repo());
    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert_eq!(builds.load(Ordering::Relaxed), 0);
    assert_eq!(game.read(DEPLOYED), shipped(&miniature_repo(), SHIPPED));

    // Now compile it, over the top of the one just deployed.
    let (core, builds) = core_at_a_release(&game, miniature_repo());
    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::AlwaysCompile),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert_eq!(builds.load(Ordering::Relaxed), 1);
    assert_eq!(game.read(DEPLOYED), DLL_MARKER);

    // And back again: the Shipped DLL returns, still without a build.
    let (core, builds) = core_at_a_release(&game, miniature_repo());
    let plan = core
        .plan(
            &a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent),
            &game.folders(),
        )
        .unwrap();
    core.execute(&plan, &ProgressReporter::silent()).unwrap();
    assert_eq!(builds.load(Ordering::Relaxed), 0);
    assert_eq!(game.read(DEPLOYED), shipped(&miniature_repo(), SHIPPED));
}

/// A second Release install skips even the copy: the fingerprint sidecar still matches.
#[test]
fn a_repeat_release_install_reuses_what_is_already_deployed() {
    let game = GameFixture::new();
    for _ in 0..2 {
        let (core, builds) = core_at_a_release(&game, miniature_repo());
        let plan = core
            .plan(
                &a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent),
                &game.folders(),
            )
            .unwrap();
        core.execute(&plan, &ProgressReporter::silent()).unwrap();
        assert_eq!(builds.load(Ordering::Relaxed), 0);
    }
    assert_eq!(game.read(DEPLOYED), shipped(&miniature_repo(), SHIPPED));
}

/// Which configurations will have to compile - the question the shell asks before the click,
/// to decide whether the Toolchain Bootstrap's download is part of the price.
///
/// Note the Arbitrary Ref: it might name a Release commit, but nothing here can know that
/// without resolving it, so it answers yes. A warning shown and not needed costs a sentence;
/// a warning needed and not shown costs a player 2.4 GB they were never told about.
#[test]
fn what_needs_the_toolchain_is_answered_before_anything_is_fetched() {
    let release = a_release(FortyThreeCivs::Disabled, DllSource::ShippedWhenCurrent);
    assert!(!release.needs_the_toolchain());

    let mut compiling = release.clone();
    compiling.dll_source = DllSource::AlwaysCompile;
    assert!(compiling.needs_the_toolchain());

    // The Replaced File is compiled every time it is asked for; no repository ships one.
    let mut with_luajit = release.clone();
    with_luajit.luajit = LuaJitEngine::LuaJit;
    assert!(with_luajit.needs_the_toolchain());

    for version in [
        Version::LatestDevelopmentVersion,
        Version::ArbitraryRef("Release-2.0".to_owned()),
        Version::UnofficialBuild {
            label: "2.0.01".to_owned(),
            commit: "a".repeat(40),
        },
    ] {
        let mut other = release.clone();
        other.source = InstallationSource::UpstreamCache { version };
        assert!(other.needs_the_toolchain(), "{:?}", other.source);
    }

    let mut local = release.clone();
    local.source = InstallationSource::LocalRepo {
        path: miniature_repo(),
    };
    assert!(local.needs_the_toolchain());
}
