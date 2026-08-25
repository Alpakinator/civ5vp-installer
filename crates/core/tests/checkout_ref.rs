//! Reading which ref a Dev-mode checkout is on.

use std::fs;

use civ5vp_core::{CheckoutRef, current_ref};

fn repo(dir: &std::path::Path, head: &str) {
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join(".git").join("HEAD"), head).unwrap();
}

#[test]
fn an_ordinary_branch_is_named() {
    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "ref: refs/heads/ai-overhaul\n");

    let found = current_ref(dir.path());

    assert_eq!(found, Some(CheckoutRef::Branch("ai-overhaul".to_owned())));
    assert_eq!(found.unwrap().describe(), "Branch ai-overhaul");
}

/// Branch names may contain slashes, so the name is everything after `refs/heads/` rather
/// than the last path segment.
#[test]
fn a_branch_name_with_slashes_survives_whole() {
    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "ref: refs/heads/feature/fix-sprintf-s\n");

    assert_eq!(
        current_ref(dir.path()),
        Some(CheckoutRef::Branch("feature/fix-sprintf-s".to_owned()))
    );
}

/// Mid-rebase, mid-bisect, or a deliberate checkout of a commit. Saying "no branch" beats
/// saying nothing, because someone who thinks they are on a branch is about to build
/// something other than what they expect.
#[test]
fn a_detached_head_says_so_rather_than_going_quiet() {
    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "0febdfe94a1b2c3d4e5f60718293a4b5c6d7e8f9\n");

    let found = current_ref(dir.path()).unwrap();

    assert_eq!(found, CheckoutRef::Detached("0febdfe".to_owned()));
    assert!(found.describe().contains("detached at 0febdfe"));
}

/// A linked worktree - which is how this project's own fork work is done - keeps `.git` as a
/// file pointing elsewhere. Treating that as "not a checkout" would report nothing for
/// exactly the setup a developer is most likely to have.
#[test]
fn a_worktree_whose_git_is_a_file_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real-git-dir");
    fs::create_dir_all(&real).unwrap();
    fs::write(real.join("HEAD"), "ref: refs/heads/fix-sprintf-s-inlining\n").unwrap();

    let tree = dir.path().join("worktree");
    fs::create_dir_all(&tree).unwrap();
    fs::write(
        tree.join(".git"),
        format!("gitdir: {}\n", real.display()),
    )
    .unwrap();

    assert_eq!(
        current_ref(&tree),
        Some(CheckoutRef::Branch("fix-sprintf-s-inlining".to_owned()))
    );
}

/// A source tree unpacked from an archive is a perfectly good Local Repo. Nothing to report
/// is an answer, not a failure.
#[test]
fn a_tree_that_is_not_a_checkout_reports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(current_ref(dir.path()), None);
    assert_eq!(current_ref(&dir.path().join("nowhere")), None);
}

/// A HEAD that is neither a ref nor a commit id is damaged. Reporting nothing beats
/// reporting something that looks like a branch name and is not.
#[test]
fn a_damaged_head_reports_nothing_rather_than_guessing() {
    let dir = tempfile::tempdir().unwrap();
    repo(dir.path(), "not a ref and not a sha\n");
    assert_eq!(current_ref(dir.path()), None);

    repo(dir.path(), "");
    assert_eq!(current_ref(dir.path()), None);
}
