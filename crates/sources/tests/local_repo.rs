//! A Local Repo is used byte-for-byte as it is (user story 29).

mod support;

use std::fs;

use civ5vp_core::{InstallationSource, ProgressReporter, SourceProvider};
use civ5vp_sources::{InstallationSources, UpstreamCache};
use support::UpstreamFixture;

/// A provider whose Upstream Cache is never reached — these tests only use the Local Repo arm.
fn sources(fixture: &UpstreamFixture) -> InstallationSources {
    InstallationSources::new(UpstreamCache::new(
        fixture.cache_root(),
        fixture.unreachable_url(),
    ))
}

#[test]
fn a_local_repo_is_handed_back_untouched_with_its_uncommitted_changes() {
    let fixture = UpstreamFixture::new();
    let checkout = fixture
        .cache_root()
        .parent()
        .unwrap()
        .join("developer-repo");
    fs::create_dir_all(checkout.join("(1) Community Patch")).unwrap();
    fs::create_dir_all(checkout.join(".git")).unwrap();
    fs::write(checkout.join(".git/HEAD"), "ref: refs/heads/my-branch\n").unwrap();
    fs::write(
        checkout.join("(1) Community Patch/(1) Community Patch.modinfo"),
        "edited but not committed",
    )
    .unwrap();

    let root = sources(&fixture)
        .materialize(
            &InstallationSource::LocalRepo {
                path: checkout.clone(),
            },
            &ProgressReporter::silent(),
        )
        .unwrap();

    assert_eq!(root, checkout);
    assert_eq!(
        fs::read_to_string(root.join("(1) Community Patch/(1) Community Patch.modinfo")).unwrap(),
        "edited but not committed"
    );
    // Nothing checked out, stashed, or cleaned: the repository is exactly as it was.
    assert_eq!(
        fs::read_to_string(root.join(".git/HEAD")).unwrap(),
        "ref: refs/heads/my-branch\n"
    );
}

#[test]
fn a_local_repo_that_is_not_a_folder_is_refused_before_anything_happens() {
    let fixture = UpstreamFixture::new();
    let missing = fixture.cache_root().parent().unwrap().join("nowhere");

    let failure = sources(&fixture)
        .materialize(
            &InstallationSource::LocalRepo {
                path: missing.clone(),
            },
            &ProgressReporter::silent(),
        )
        .unwrap_err();

    assert!(
        failure.message().contains("There is no folder at"),
        "unexpected message: {}",
        failure.message()
    );
    assert!(failure.detail().contains("NotADirectory"));
}

#[test]
fn a_relative_local_repo_path_is_refused() {
    let fixture = UpstreamFixture::new();

    let failure = sources(&fixture)
        .materialize(
            &InstallationSource::LocalRepo {
                path: "../Community-Patch-DLL".into(),
            },
            &ProgressReporter::silent(),
        )
        .unwrap_err();

    assert!(
        failure.message().contains("full path"),
        "unexpected message: {}",
        failure.message()
    );
}
