//! A Local Repo is used byte-for-byte as it is.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use civ5vp_core::{InstallationSource, ProgressReporter, SourceProvider};
use civ5vp_sources::{InstallationSources, LuaJitCache, UpstreamCache};
use support::UpstreamFixture;

/// A provider whose Upstream Cache is never reached - these tests only use the Local Repo arm.
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

/// A folder that exists but is not the repository - a Steam library, a mods folder - is
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

/// Run the real `git` - the one the fast suite already needs for its fixtures - in `path`,
/// away from this machine's own git configuration, and return what it printed.
fn git(path: &Path, args: &[&str]) -> String {
    let no_config = path.parent().unwrap().join("no-gitconfig");
    fs::write(&no_config, "").unwrap();
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", &no_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// A real checkout with one commit, made by git itself, so what the installer calls it can
/// be held against what git says.
fn real_checkout(fixture: &UpstreamFixture, name: &str) -> PathBuf {
    let path = fixture.cache_root().parent().unwrap().join(name);
    fs::create_dir_all(path.join("CvGameCoreDLL_Expansion2")).unwrap();
    git(&path, &["init", "-q"]);
    commit_file(&path, "CvGameCoreDLL_Expansion2/CvGame.cpp", "// first\n");
    path
}

fn commit_file(checkout: &Path, file: &str, contents: &str) {
    fs::write(checkout.join(file), contents).unwrap();
    git(checkout, &["add", "-A"]);
    git(checkout, &["commit", "-q", "-m", contents]);
}

/// What the Local Repo provider says the DLL should call itself, and everything it reported.
fn describe(fixture: &UpstreamFixture, checkout: &Path) -> (Option<String>, Vec<String>) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let materialized = sources(fixture)
        .materialize(
            &InstallationSource::LocalRepo {
                path: checkout.to_path_buf(),
            },
            &ProgressReporter::to_channel(sender),
        )
        .unwrap();
    let reported = receiver.try_iter().map(|event| event.message).collect();
    (materialized.git_description, reported)
}

/// The name upstream's own build gives a DLL: `git describe --tags --long`, lightweight tags
/// included, then Clean or Dirty - so a crash report says which commit crashed.
#[test]
fn a_local_repo_is_named_the_way_git_describe_names_it() {
    let fixture = UpstreamFixture::new();
    let checkout = real_checkout(&fixture, "described-repo");
    git(&checkout, &["tag", "Release-1.0"]);
    commit_file(
        &checkout,
        "CvGameCoreDLL_Expansion2/CvGame.cpp",
        "// second\n",
    );
    commit_file(
        &checkout,
        "CvGameCoreDLL_Expansion2/CvGame.cpp",
        "// third\n",
    );

    let (description, reported) = describe(&fixture, &checkout);

    let git_says = git(&checkout, &["describe", "--tags", "--long"]);
    assert!(git_says.starts_with("Release-1.0-2-g"), "{git_says}");
    assert_eq!(description, Some(format!("{git_says} Clean")));
    assert!(
        reported.iter().any(|line| line.contains(&git_says)),
        "the name must be reported: {reported:?}"
    );
}

/// Dirty means a tracked file differs from `HEAD`, staged or not. An untracked file does
/// not count - upstream's script asks git with `--untracked-files=no`.
#[test]
fn changed_tracked_files_make_a_checkout_dirty_and_untracked_files_do_not() {
    let fixture = UpstreamFixture::new();
    let checkout = real_checkout(&fixture, "dirty-checkout");
    git(&checkout, &["tag", "Release-1.0"]);
    let name = |fixture| describe(fixture, &checkout).0.unwrap();

    fs::write(checkout.join("notes.txt"), "untracked").unwrap();
    assert!(name(&fixture).ends_with(" Clean"), "{}", name(&fixture));

    fs::write(
        checkout.join("CvGameCoreDLL_Expansion2/CvGame.cpp"),
        "// edited\n",
    )
    .unwrap();
    assert_eq!(
        name(&fixture),
        "Release-1.0-0-g".to_owned() + &short_head(&checkout) + " Dirty"
    );

    git(
        &checkout,
        &["checkout", "--", "CvGameCoreDLL_Expansion2/CvGame.cpp"],
    );
    git(&checkout, &["add", "notes.txt"]);
    assert!(
        name(&fixture).ends_with(" Dirty"),
        "a staged file is a change too: {}",
        name(&fixture)
    );
}

fn short_head(checkout: &Path) -> String {
    git(checkout, &["rev-parse", "--short", "HEAD"])
}

/// With no tag behind it, the commit alone names the checkout - upstream's fallback wording.
#[test]
fn a_checkout_with_no_tag_is_named_by_its_commit() {
    let fixture = UpstreamFixture::new();
    let checkout = real_checkout(&fixture, "untagged-repo");

    let (description, _) = describe(&fixture, &checkout);

    assert_eq!(
        description,
        Some(format!("No-Tag {} Clean", short_head(&checkout)))
    );
}

/// Asking git is a read. Even when every file's timestamp is newer than the index records -
/// the case where `git status` rewrites `.git/index` - nothing in the checkout changes.
#[test]
fn naming_a_checkout_writes_nothing_into_it() {
    let fixture = UpstreamFixture::new();
    let checkout = real_checkout(&fixture, "read-only-repo");
    let index = checkout.join(".git/index");
    // Same bytes, new timestamp: git has to look inside the file to call it unchanged.
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(
        checkout.join("CvGameCoreDLL_Expansion2/CvGame.cpp"),
        "// first\n",
    )
    .unwrap();
    let index_before = fs::read(&index).unwrap();
    let modified_before = fs::metadata(&index).unwrap().modified().unwrap();

    let (description, _) = describe(&fixture, &checkout);

    assert!(description.unwrap().ends_with(" Clean"));
    assert_eq!(fs::read(&index).unwrap(), index_before);
    assert_eq!(
        fs::metadata(&index).unwrap().modified().unwrap(),
        modified_before
    );
}

/// A tree git cannot read still builds, under the name it always had - and says why.
#[test]
fn a_local_repo_git_cannot_read_is_built_as_local_and_says_so() {
    let fixture = UpstreamFixture::new();
    let not_git = fixture.cache_root().parent().unwrap().join("exported-tree");
    fs::create_dir_all(not_git.join("CvGameCoreDLL_Expansion2")).unwrap();

    let (description, reported) = describe(&fixture, &not_git);

    assert_eq!(description, None);
    assert!(
        reported.iter().any(|line| line.contains("\"Local\"")),
        "the fallback must be reported: {reported:?}"
    );
}
