//! The Upstream Cache, against fixture repositories rather than the network (rule 13).
//!
//! Everything here goes through the crate's public API: list the Versions, materialize one,
//! look at the files that appear. The real-upstream counterparts live in `real_upstream.rs`
//! and are `#[ignore]`d (rule 14).

mod support;

use civ5vp_core::{ProgressReporter, Version};
use civ5vp_sources::UpstreamCache;
use support::{UpstreamFixture, materialized_files};

/// Read a file out of the materialized tree.
fn read(root: &std::path::Path, relative: &str) -> String {
    std::fs::read_to_string(root.join(relative)).unwrap()
}

#[test]
fn version_picker_lists_releases_newest_first() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let catalog = cache.list_versions(&ProgressReporter::silent()).unwrap();

    assert_eq!(catalog.releases(), fixture.release_tags());
    assert_eq!(
        catalog.newest_release(),
        Some(Version::Release("Release-2.0".to_owned()))
    );
}

#[test]
fn version_picker_reports_the_latest_development_version() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let catalog = cache.list_versions(&ProgressReporter::silent()).unwrap();

    assert_eq!(catalog.latest_development_version(), fixture.master_head());
}

#[test]
fn materializing_a_release_checks_out_that_releases_files() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let root = cache
        .materialize(
            &Version::Release("Release-2.0".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();

    assert_eq!(
        materialized_files(&root),
        [
            "(1) Community Patch/(1) Community Patch.modinfo",
            "(1) Community Patch/Kit/ReadMe.txt",
            "(2) Vox Populi/(2) Vox Populi.modinfo",
        ]
    );
    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "2.0"
    );
}

#[test]
fn a_release_can_be_named_without_its_prefix() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let root = cache
        .materialize(
            &Version::Release("2.0".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();

    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "2.0"
    );
}

#[test]
fn materializing_the_latest_development_version_checks_out_master_head() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let root = cache
        .materialize(
            &Version::LatestDevelopmentVersion,
            &ProgressReporter::silent(),
        )
        .unwrap();

    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "master"
    );
}

#[test]
fn an_arbitrary_ref_accepts_a_branch_name() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let root = cache
        .materialize(
            &Version::ArbitraryRef("experimental".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();

    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "2.0"
    );
}

#[test]
fn an_arbitrary_ref_accepts_a_tag_name_that_is_not_a_release() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    let root = cache
        .materialize(
            &Version::ArbitraryRef("experiment-42".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();

    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "2.0"
    );
}

#[test]
fn switching_version_leaves_no_file_from_the_previous_version() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    cache
        .materialize(
            &Version::Release("Release-1.0".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();
    let root = cache
        .materialize(
            &Version::Release("Release-2.0".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();

    // `RetiredInLaterVersions.txt` exists only in Release-1.0.
    assert_eq!(
        materialized_files(&root),
        [
            "(1) Community Patch/(1) Community Patch.modinfo",
            "(1) Community Patch/Kit/ReadMe.txt",
            "(2) Vox Populi/(2) Vox Populi.modinfo",
        ]
    );
}

#[test]
fn switching_back_to_an_earlier_version_restores_its_files() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());

    for version in ["Release-1.0", "Release-2.0", "Release-1.0"] {
        cache
            .materialize(
                &Version::Release(version.to_owned()),
                &ProgressReporter::silent(),
            )
            .unwrap();
    }
    let root = cache.root();

    assert_eq!(
        materialized_files(root),
        [
            "(1) Community Patch/(1) Community Patch.modinfo",
            "(1) Community Patch/Kit/ReadMe.txt",
            "(1) Community Patch/RetiredInLaterVersions.txt",
        ]
    );
}

#[test]
fn materializing_the_same_version_twice_is_idempotent() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());
    let version = Version::Release("Release-2.0".to_owned());

    let first = cache
        .materialize(&version, &ProgressReporter::silent())
        .unwrap();
    let before = materialized_files(&first);
    let second = cache
        .materialize(&version, &ProgressReporter::silent())
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(before, materialized_files(&second));
}

#[test]
fn an_unknown_version_is_reported_as_such_and_the_cache_still_works() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());
    cache
        .materialize(
            &Version::Release("Release-2.0".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();

    let failure = cache
        .materialize(
            &Version::Release("Release-9.9".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap_err();

    assert!(
        failure.user_message().contains("Release-9.9"),
        "the message should name the version the user asked for: {}",
        failure.user_message()
    );
    // The Version that was already there is untouched, and asking for it again still works.
    let root = cache
        .materialize(
            &Version::Release("Release-2.0".to_owned()),
            &ProgressReporter::silent(),
        )
        .unwrap();
    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "2.0"
    );
}

#[test]
fn a_failed_fetch_leaves_the_cache_consistent_and_a_retry_succeeds() {
    let fixture = UpstreamFixture::new();
    let version = Version::Release("Release-2.0".to_owned());

    // The upstream is unreachable — the cache directory may be created, but nothing else.
    let offline = UpstreamCache::with_url(fixture.cache_root(), fixture.unreachable_url());
    let failure = offline
        .materialize(&version, &ProgressReporter::silent())
        .unwrap_err();
    assert!(
        failure.user_message().contains("internet connection"),
        "a network failure should read like one: {}",
        failure.user_message()
    );
    assert_eq!(
        materialized_files(&fixture.cache_root()),
        Vec::<String>::new()
    );

    // Same cache directory, upstream back: the retry has to work rather than trip over
    // whatever the failed attempt left behind.
    let online = UpstreamCache::with_url(fixture.cache_root(), fixture.url());
    let root = online
        .materialize(&version, &ProgressReporter::silent())
        .unwrap();
    assert_eq!(
        read(&root, "(1) Community Patch/(1) Community Patch.modinfo"),
        "2.0"
    );
}

#[test]
fn a_checkout_that_did_not_finish_is_redone_rather_than_trusted() {
    let fixture = UpstreamFixture::new();
    let cache = UpstreamCache::with_url(fixture.cache_root(), fixture.url());
    let version = Version::Release("Release-2.0".to_owned());
    let root = cache
        .materialize(&version, &ProgressReporter::silent())
        .unwrap();

    // Exactly the state an interrupted checkout leaves: files half there, nothing recorded
    // as materialized.
    std::fs::remove_file(root.join(".git/civ5vp-materialized")).unwrap();
    std::fs::remove_dir_all(root.join("(2) Vox Populi")).unwrap();

    let root = cache
        .materialize(&version, &ProgressReporter::silent())
        .unwrap();

    assert_eq!(
        materialized_files(&root),
        [
            "(1) Community Patch/(1) Community Patch.modinfo",
            "(1) Community Patch/Kit/ReadMe.txt",
            "(2) Vox Populi/(2) Vox Populi.modinfo",
        ]
    );
}
