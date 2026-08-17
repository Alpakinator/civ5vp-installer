//! A Local Repo is used byte-for-byte as it is.

mod support;

use std::fs;

use civ5vp_core::{InstallationSource, ProgressReporter, SourceProvider};
use civ5vp_sources::{InstallationSources, LuaJitCache, UpstreamCache};
use support::UpstreamFixture;

/// A provider whose Upstream Cache is never reached — these tests only use the Local Repo arm.
fn sources(fixture: &UpstreamFixture) -> InstallationSources {
    InstallationSources::new(
        UpstreamCache::new(fixture.cache_root(), fixture.unreachable_url()),
        LuaJitCache::new(fixture.cache_root().join("luajit")),
    )
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
    fs::create_dir_all(checkout.join("CvGameCoreDLL_Expansion2")).unwrap();
    fs::write(
        checkout.join("CvGameCoreDLL_Expansion2/CvGame.cpp"),
        "// wip",
    )
    .unwrap();
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
        .unwrap()
        .root;

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

/// A folder that exists but is not the repository — a Steam library, a mods folder — is
/// named as such, before any Deployment starts.
#[test]
fn a_folder_that_is_not_a_checkout_is_refused_with_a_sentence() {
    let fixture = UpstreamFixture::new();
    let not_a_repo = fixture.cache_root().parent().unwrap().join("random-folder");
    fs::create_dir_all(&not_a_repo).unwrap();

    let failure = sources(&fixture)
        .materialize(
            &InstallationSource::LocalRepo {
                path: not_a_repo.clone(),
            },
            &ProgressReporter::silent(),
        )
        .unwrap_err();

    assert!(
        failure
            .message()
            .contains("not a Community-Patch-DLL repository"),
        "unexpected message: {}",
        failure.message()
    );
    assert!(failure.detail().contains("NotACheckout"));
}

/// The dirty working tree names itself by content: editing a DLL source changes the
/// identity, and the read is the only thing that happens to the tree.
#[test]
fn a_dirty_local_repos_identity_tracks_its_working_files() {
    let fixture = UpstreamFixture::new();
    let checkout = fixture.cache_root().parent().unwrap().join("dirty-repo");
    fs::create_dir_all(checkout.join("(1) Community Patch")).unwrap();
    fs::create_dir_all(checkout.join("CvGameCoreDLL_Expansion2")).unwrap();
    fs::write(
        checkout.join("CvGameCoreDLL_Expansion2/CvGame.cpp"),
        "// v1",
    )
    .unwrap();
    let source = InstallationSource::LocalRepo {
        path: checkout.clone(),
    };

    let before = sources(&fixture)
        .materialize(&source, &ProgressReporter::silent())
        .unwrap();
    let unchanged = sources(&fixture)
        .materialize(&source, &ProgressReporter::silent())
        .unwrap();
    fs::write(
        checkout.join("CvGameCoreDLL_Expansion2/CvGame.cpp"),
        "// v2",
    )
    .unwrap();
    let edited = sources(&fixture)
        .materialize(&source, &ProgressReporter::silent())
        .unwrap();

    assert!(before.source_identity.starts_with("files "));
    assert_eq!(before.source_identity, unchanged.source_identity);
    assert_ne!(before.source_identity, edited.source_identity);
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
