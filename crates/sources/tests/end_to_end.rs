//! The Upstream Cache driving a real Deployment through the Core seam.
//!
//! The source provider here is the real one — it fetches and checks out a Version — while the
//! toolchain runner is still faked. This is the half of "a real Release installs end-to-end"
//! that can run per-commit; the other half, against the real upstream repository, is the
//! `#[ignore]`d test in `real_upstream.rs`.

mod support;

use std::fs;
use std::path::PathBuf;

use civ5vp_core::{
    BoundaryError, BuildConfiguration, BuildRequest, Core, Flavor, FortyThreeCivs, GameFolders,
    InstallConfiguration, InstallMode, InstallationSource, ProgressReporter, Stage,
    ToolchainRunner, Version,
};
use civ5vp_sources::{InstallationSources, UpstreamCache};
use support::UpstreamFixture;

/// What the fake toolchain runner writes where the Built DLL would go.
pub const DLL_MARKER: &str = "marker artifact standing in for the Built DLL";

struct MarkerToolchainRunner;

impl ToolchainRunner for MarkerToolchainRunner {
    fn build_dll(
        &self,
        request: &BuildRequest,
        progress: &ProgressReporter,
    ) -> Result<(), BoundaryError> {
        progress.report(Stage::Build, "Faking a DLL build.");
        fs::write(&request.output_path, DLL_MARKER).map_err(|err| {
            BoundaryError::new("The DLL build failed.", format!("fake runner: {err}"))
        })
    }

    fn toolchain_identity(&self) -> String {
        "fake-toolchain-0".to_owned()
    }
}

struct GameFixture {
    root: PathBuf,
}

impl GameFixture {
    fn new(root: PathBuf) -> Self {
        for folder in ["MODS", "DLC", "Text", "cache"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        Self { root }
    }

    fn folders(&self) -> GameFolders {
        GameFolders {
            mods: self.root.join("MODS"),
            dlc: self.root.join("DLC"),
            text: self.root.join("Text"),
        }
    }
}

#[test]
fn a_release_from_the_upstream_cache_installs_end_to_end() {
    let fixture = UpstreamFixture::new();
    let app_data = fixture.cache_root().parent().unwrap().to_path_buf();
    let game = GameFixture::new(app_data.join("game"));

    let core = Core::new(
        Box::new(InstallationSources::new(UpstreamCache::new(
            fixture.cache_root(),
            fixture.url(),
        ))),
        Box::new(MarkerToolchainRunner),
        Box::new(UnusedModpackAssembler),
        app_data.join("work"),
    );
    let configuration = InstallConfiguration {
        source: InstallationSource::UpstreamCache {
            version: Version::Release("Release-2.0".to_owned()),
        },
        flavor: Flavor::CommunityPatch,
        forty_three_civs: FortyThreeCivs::Disabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
    };

    let plan = core.plan(&configuration, &game.folders()).unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    let installed = game.root.join("MODS/(1) Community Patch");
    assert_eq!(
        fs::read_to_string(installed.join("(1) Community Patch.modinfo")).unwrap(),
        "2.0"
    );
    assert_eq!(
        fs::read_to_string(installed.join("Kit/ReadMe.txt")).unwrap(),
        "kit"
    );
    assert_eq!(
        fs::read_to_string(&outcome.built_dll).unwrap(),
        DLL_MARKER,
        "the Built DLL, not the repository's"
    );
}

#[test]
fn switching_version_between_installs_removes_what_the_new_version_dropped() {
    let fixture = UpstreamFixture::new();
    let app_data = fixture.cache_root().parent().unwrap().to_path_buf();
    let game = GameFixture::new(app_data.join("game"));

    let core = Core::new(
        Box::new(InstallationSources::new(UpstreamCache::new(
            fixture.cache_root(),
            fixture.url(),
        ))),
        Box::new(MarkerToolchainRunner),
        Box::new(UnusedModpackAssembler),
        app_data.join("work"),
    );
    let install = |version: &str| {
        let configuration = InstallConfiguration {
            source: InstallationSource::UpstreamCache {
                version: Version::Release(version.to_owned()),
            },
            flavor: Flavor::CommunityPatch,
            forty_three_civs: FortyThreeCivs::Disabled,
            build_configuration: BuildConfiguration::Release,
            install_mode: InstallMode::Mods,
            extra_mods: Vec::new(),
        };
        let plan = core.plan(&configuration, &game.folders()).unwrap();
        core.execute(&plan, &ProgressReporter::silent()).unwrap();
    };

    install("Release-1.0");
    let retired = game
        .root
        .join("MODS/(1) Community Patch/RetiredInLaterVersions.txt");
    assert!(retired.is_file(), "the older Release ships this file");

    install("Release-2.0");

    assert!(
        !retired.exists(),
        "a file the new Version dropped must not survive in the game folder"
    );
    assert_eq!(
        fs::read_to_string(
            game.root
                .join("MODS/(1) Community Patch/(1) Community Patch.modinfo")
        )
        .unwrap(),
        "2.0"
    );
}

/// For tests that never run a Modpack Deployment: refuses if asked.
struct UnusedModpackAssembler;

impl civ5vp_core::ModpackAssembler for UnusedModpackAssembler {
    fn cache_state(
        &self,
        _gameplay_db: &std::path::Path,
    ) -> Result<civ5vp_core::CacheState, civ5vp_core::BoundaryError> {
        Err(civ5vp_core::BoundaryError::new(
            "No modpack in this test.",
            "UnusedModpackAssembler::cache_state called",
        ))
    }

    fn merge_and_dump(
        &self,
        _job: &civ5vp_core::ModpackDatabaseJob,
        _progress: &civ5vp_core::ProgressReporter,
    ) -> Result<(), civ5vp_core::BoundaryError> {
        Err(civ5vp_core::BoundaryError::new(
            "No modpack in this test.",
            "UnusedModpackAssembler::merge_and_dump called",
        ))
    }
}
