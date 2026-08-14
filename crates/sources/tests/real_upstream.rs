//! Real-clone integration tests against `LoneGazebo/Community-Patch-DLL`.
//!
//! `#[ignore]`d, never deleted (rule 14): they download hundreds of megabytes and take
//! minutes. Run them with
//!
//! ```text
//! cargo test -p civ5vp-sources --test real_upstream -- --ignored --nocapture --test-threads 1
//! ```
//!
//! `--nocapture` matters: the measurements these tests assert on are also printed, and the
//! printed numbers are the evidence behind the transfer budget in ticket 04.
//!
//! ## What "transferred" means here
//!
//! Each test measures how much the Upstream Cache's object store grew. That is an *upper
//! bound* on the bytes that came over the wire: the received pack is written to disk as it
//! arrives, and thin-pack completion can only add to it. So a measurement below a ceiling
//! proves the wire traffic was below it too.

use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{
    BoundaryError, BuildConfiguration, BuildRequest, Core, Eui, Flavor, FortyThreeCivs,
    GameFolders, InstallConfiguration, InstallMode, InstallationSource, ProgressReporter, Stage,
    ToolchainRunner, Version,
};
use civ5vp_sources::{InstallationSources, UPSTREAM_URL, UpstreamCache};

/// Ticket 04's ceiling for a first materialization.
///
/// Measured on 2026-08-03: 147.7 MiB. A shallow fetch transfers a *snapshot*, so this figure
/// tracks how big the mod is, not how long its history is — it creeps up as files are added.
/// 200 MiB is roughly a third above today's figure: enough that ordinary growth does not fail
/// the test, tight enough that a change of strategy back towards full history would.
const FIRST_FETCH_CEILING: u64 = 200 * 1024 * 1024;

/// Ticket 04's ceiling for switching to another Version.
///
/// Measured on 2026-08-03: 32.7 MiB for `master` → `Release-4.15`, roughly a year apart.
///
/// The margin is *not* for Versions further apart: ADR-0004's own measurements show the
/// opposite, with the furthest switch measured (`Release-3.0`) costing 9.0 MiB, because an
/// older Release is a smaller snapshot. What a switch actually costs is how much of the target
/// snapshot the cache does not already hold, so the worst case is a switch to a *newer,
/// larger* Version — which is what 50 MiB leaves room for.
const VERSION_SWITCH_CEILING: u64 = 50 * 1024 * 1024;

/// A Release far enough back that switching to it is a real switch, not a no-op.
const OLDER_RELEASE: &str = "Release-4.15";

/// A directory under `target/`, so a half-gigabyte of git objects does not land in `/tmp`,
/// which is a RAM disk on the machines this is developed on.
fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

/// Bytes in the cache's object store — an upper bound on what was transferred into it.
fn object_store_bytes(root: &Path) -> u64 {
    fn walk(dir: &Path) -> u64 {
        let Ok(entries) = fs::read_dir(dir) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| match entry.metadata() {
                Ok(metadata) if metadata.is_dir() => walk(&entry.path()),
                Ok(metadata) => metadata.len(),
                Err(_) => 0,
            })
            .sum()
    }
    walk(&root.join(".git/objects"))
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

#[test]
#[ignore = "clones the real upstream repository"]
fn the_version_picker_lists_the_real_releases_and_master() {
    let cache = UpstreamCache::new(scratch("real-list"), UPSTREAM_URL);

    let catalog = cache.list_versions(&ProgressReporter::silent()).unwrap();

    println!(
        "releases: {} — newest {:?}",
        catalog.releases().len(),
        catalog.releases().first()
    );
    println!("master: {}", catalog.latest_development_version());
    assert!(
        catalog.releases().len() > 100,
        "upstream has had a lot of releases, got {}",
        catalog.releases().len()
    );
    assert!(
        catalog.releases().iter().any(|tag| tag == "Release-5.4.2"),
        "a known Release should be listed"
    );
    assert_eq!(
        catalog.latest_development_version().len(),
        40,
        "master should resolve to a commit id"
    );
    // Newest first: the picker's default selection is the top of the list.
    let newest = catalog.newest_release().unwrap();
    assert!(
        matches!(newest, Version::Release(ref tag) if tag.starts_with("Release-5.")),
        "unexpected newest release: {newest:?}"
    );
}

/// Ticket 04's transfer budget, measured rather than estimated.
#[test]
#[ignore = "clones the real upstream repository"]
fn a_first_materialization_and_a_version_switch_stay_within_budget() {
    let root = scratch("real-budget");
    let cache = UpstreamCache::new(&root, UPSTREAM_URL);

    let started = std::time::Instant::now();
    cache
        .materialize(
            &Version::LatestDevelopmentVersion,
            &ProgressReporter::silent(),
        )
        .unwrap();
    let first = object_store_bytes(&root);
    println!(
        "first materialization (master): {} in {:.0}s",
        mib(first),
        started.elapsed().as_secs_f64()
    );

    let started = std::time::Instant::now();
    cache
        .materialize(
            &Version::Release(OLDER_RELEASE.to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();
    let switch = object_store_bytes(&root) - first;
    println!(
        "version switch (master -> {OLDER_RELEASE}): {} in {:.0}s",
        mib(switch),
        started.elapsed().as_secs_f64()
    );

    // The switch has to bring back a working tree, so this also proves the second fetch was
    // an increment rather than a second full snapshot.
    assert!(
        root.join("(1) Community Patch").is_dir(),
        "the switched-to Version should be on disk"
    );
    assert!(
        first < FIRST_FETCH_CEILING,
        "first materialization transferred {}, ceiling {}",
        mib(first),
        mib(FIRST_FETCH_CEILING)
    );
    assert!(
        switch < VERSION_SWITCH_CEILING,
        "version switch transferred {}, ceiling {}",
        mib(switch),
        mib(VERSION_SWITCH_CEILING)
    );
}

#[test]
#[ignore = "clones the real upstream repository"]
fn an_arbitrary_ref_can_be_a_commit_id() {
    let root = scratch("real-arbitrary");
    let cache = UpstreamCache::new(&root, UPSTREAM_URL);
    // The commit `Release-5.4.2` peels to.
    let commit = "8e180cfd1cb7fc354abc0d6d7b23e2602dd2d3db";

    let materialized = cache
        .materialize(
            &Version::ArbitraryRef(commit.to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap()
        .root;

    assert!(materialized.join("(1) Community Patch").is_dir());
}

#[test]
#[ignore = "clones the real upstream repository"]
fn a_real_release_installs_end_to_end() {
    let root = scratch("real-install");
    let game_root = root.join("game");
    for folder in ["MODS", "DLC", "Text", "cache"] {
        fs::create_dir_all(game_root.join(folder)).unwrap();
    }
    let folders = GameFolders {
        mods: game_root.join("MODS"),
        dlc: game_root.join("DLC"),
        text: game_root.join("Text"),
    };

    let core = Core::new(
        Box::new(InstallationSources::new(UpstreamCache::new(
            root.join("upstream-cache"),
            UPSTREAM_URL,
        ))),
        Box::new(MarkerToolchainRunner),
        Box::new(UnusedModpackAssembler),
        root.join("work"),
    );
    let configuration = InstallConfiguration {
        source: InstallationSource::UpstreamCache {
            version: Version::Release("Release-5.4.2".to_owned()),
        },
        // The whole Vox Populi Flavor, so every folder the Deployment matrix needs has to
        // really be present at that Release.
        flavor: Flavor::VoxPopuli { eui: Eui::Enabled },
        forty_three_civs: FortyThreeCivs::Enabled,
        build_configuration: BuildConfiguration::Release,
        install_mode: InstallMode::Mods,
        extra_mods: Vec::new(),
    };

    let plan = core.plan(&configuration, &folders).unwrap();
    let outcome = core.execute(&plan, &ProgressReporter::silent()).unwrap();

    println!("deployed: {:?}", outcome.deployed);
    // Upstream puts the mod's version in the file name — `(1) Community Patch (v 151).modinfo`
    // at the time of writing — so the extension is what can be asserted on.
    let community_patch = game_root.join("MODS/(1) Community Patch");
    assert!(
        fs::read_dir(&community_patch).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "modinfo")
        }),
        "the Community Patch should have been deployed with its modinfo"
    );
    assert!(game_root.join("DLC/UI_bc1").is_dir());
    assert!(game_root.join("Text/VPUI_tips_en_us.xml").is_file());
    assert_eq!(
        fs::read_to_string(&outcome.built_dll).unwrap(),
        DLL_MARKER,
        "the deployed DLL is the built one, never the repository's"
    );
}

const DLL_MARKER: &str = "marker artifact standing in for the Built DLL";

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

/// Ticket 11's boundary, for tests that never run a Modpack Deployment: refuses if asked.
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
