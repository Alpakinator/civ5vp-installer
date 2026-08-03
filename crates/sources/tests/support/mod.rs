//! Fixture repositories for the fast suite.
//!
//! Rule 13 keeps the per-commit suite off the network, but the interesting behaviour here —
//! which refs a Version resolves to, what a fetch brings back, what a checkout leaves on disk
//! — only exists if there is a real repository at the other end. So these tests build one:
//! real commits, real annotated and lightweight tags, real branches, written with the same
//! `gix` the installer uses, and fetched over a `file://` URL.
//!
//! One caveat worth stating plainly: `gix`'s `file://` transport spawns `git-upload-pack`, so
//! **these tests need git on the machine running them**. The installer never uses `file://` —
//! it only ever talks `https` to GitHub, which `gix` speaks in-process — so this is a
//! property of the fixtures, not of the shipped code (rule 5 is about the user's machine).

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use gix::objs::tree::{Entry, EntryKind};

/// A small Community-Patch-DLL-shaped repository with a history worth switching between.
pub struct UpstreamFixture {
    temp: tempfile::TempDir,
    releases: Vec<(String, gix::ObjectId)>,
    master_head: gix::ObjectId,
    second_release_commit: gix::ObjectId,
}

/// A file the fixture commits, as `path` and `contents`.
type File<'a> = (&'a str, &'a str);

impl UpstreamFixture {
    /// Build the fixture: three commits, four Releases, and a branch beside `master`.
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("upstream");
        fs::create_dir_all(&path).unwrap();
        gix::init(&path).unwrap();
        // Every ref the fixture writes gets a reflog entry, and a reflog entry needs an
        // identity. Giving the repository one is less intrusive than setting it in the
        // environment of the whole test process.
        fs::write(
            path.join(".git/config"),
            "[user]\n\tname = Fixture\n\temail = fixture@example.invalid\n",
        )
        .unwrap();
        let repo = gix::open(&path).unwrap();

        let first = commit(
            &repo,
            "the first release",
            &[
                ("(1) Community Patch/(1) Community Patch.modinfo", "1.0"),
                ("(1) Community Patch/Kit/ReadMe.txt", "kit"),
                ("(1) Community Patch/RetiredInLaterVersions.txt", "retired"),
            ],
            &[],
        );
        let second = commit(
            &repo,
            "the second release",
            &[
                ("(1) Community Patch/(1) Community Patch.modinfo", "2.0"),
                ("(1) Community Patch/Kit/ReadMe.txt", "kit"),
                ("(2) Vox Populi/(2) Vox Populi.modinfo", "vp 2.0"),
            ],
            &[first],
        );
        let master_head = commit(
            &repo,
            "development since the second release",
            &[
                ("(1) Community Patch/(1) Community Patch.modinfo", "master"),
                ("(1) Community Patch/Kit/ReadMe.txt", "kit"),
                ("(2) Vox Populi/(2) Vox Populi.modinfo", "vp master"),
            ],
            &[second],
        );

        set_ref(&repo, "refs/heads/master", master_head);
        // A branch that is not `master`, so an Arbitrary Ref has something to point at.
        set_ref(&repo, "refs/heads/experimental", second);

        // Ordering has to be numeric, not alphabetic: 1.10 is newer than 1.9, and "1.10"
        // sorts before "1.9" as a string.
        set_ref(&repo, "refs/tags/Release-1.0", first);
        set_ref(&repo, "refs/tags/Release-1.9", first);
        set_ref(&repo, "refs/tags/Release-1.10", first);
        // Annotated, so the checkout path has to peel a tag object to reach the commit.
        repo.tag(
            "Release-2.0",
            second,
            gix::object::Kind::Commit,
            Some(signature()),
            "the second release",
            gix::refs::transaction::PreviousValue::Any,
        )
        .unwrap();
        // Not a Release, so it must not appear in the picker.
        set_ref(&repo, "refs/tags/experiment-42", second);

        Self {
            temp,
            releases: vec![
                ("Release-2.0".to_owned(), second),
                ("Release-1.10".to_owned(), first),
                ("Release-1.9".to_owned(), first),
                ("Release-1.0".to_owned(), first),
            ],
            master_head,
            second_release_commit: second,
        }
    }

    /// The fixture's URL, as the Upstream Cache would be configured with.
    pub fn url(&self) -> String {
        format!("file://{}", self.temp.path().join("upstream").display())
    }

    /// A URL that parses but points at nothing, for the failed-fetch case.
    pub fn unreachable_url(&self) -> String {
        format!(
            "file://{}",
            self.temp.path().join("not-a-repository").display()
        )
    }

    /// Where the Upstream Cache should live — beside the fixture, not inside it.
    pub fn cache_root(&self) -> PathBuf {
        self.temp.path().join("app-data/upstream-cache")
    }

    /// Release tags newest first, the order the picker is expected to produce.
    pub fn release_tags(&self) -> Vec<String> {
        self.releases.iter().map(|(tag, _)| tag.clone()).collect()
    }

    pub fn master_head(&self) -> String {
        self.master_head.to_hex().to_string()
    }

    /// The commit `Release-2.0` and the `experimental` branch both point at.
    pub fn second_release_commit(&self) -> String {
        self.second_release_commit.to_hex().to_string()
    }
}

/// Every file under `root` except the repository itself, as `/`-separated relative paths.
pub fn materialized_files(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect(root, root, &mut found);
    found.sort();
    found
}

fn collect(root: &Path, dir: &Path, found: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, found);
        } else {
            found.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

/// A fixed identity, so the fixture's commit ids are the same on every machine.
fn signature() -> gix::actor::SignatureRef<'static> {
    gix::actor::SignatureRef {
        name: "Fixture".into(),
        email: "fixture@example.invalid".into(),
        time: "1700000000 +0000",
    }
}

/// Write `files` as a tree and commit it without touching any working directory.
fn commit(
    repo: &gix::Repository,
    message: &str,
    files: &[File<'_>],
    parents: &[gix::ObjectId],
) -> gix::ObjectId {
    let tree = write_tree(repo, "", files);
    repo.commit_as(
        signature(),
        signature(),
        "refs/heads/fixture-building",
        message,
        tree,
        parents.iter().copied(),
    )
    .unwrap()
    .detach()
}

/// Build one tree object for `prefix`, recursing into every directory below it.
fn write_tree(repo: &gix::Repository, prefix: &str, files: &[File<'_>]) -> gix::ObjectId {
    let mut entries: Vec<Entry> = Vec::new();
    let mut directories: Vec<String> = Vec::new();

    for (path, contents) in files {
        let Some(relative) = path.strip_prefix(prefix) else {
            continue;
        };
        match relative.split_once('/') {
            None => entries.push(Entry {
                mode: EntryKind::Blob.into(),
                filename: relative.into(),
                oid: repo.write_blob(contents).unwrap().detach(),
            }),
            Some((directory, _)) => {
                if !directories.iter().any(|seen| seen == directory) {
                    directories.push(directory.to_owned());
                }
            }
        }
    }
    for directory in directories {
        let nested = format!("{prefix}{directory}/");
        entries.push(Entry {
            mode: EntryKind::Tree.into(),
            filename: directory.as_str().into(),
            oid: write_tree(repo, &nested, files),
        });
    }

    // Git compares tree entry names with a `/` appended to directories; writing them in any
    // other order produces a tree git considers malformed.
    entries.sort_by_key(|entry| {
        let mut key = entry.filename.to_vec();
        if entry.mode.is_tree() {
            key.push(b'/');
        }
        key
    });
    repo.write_object(&gix::objs::Tree { entries })
        .unwrap()
        .detach()
}

fn set_ref(repo: &gix::Repository, name: &str, target: gix::ObjectId) {
    repo.reference(
        name,
        target,
        gix::refs::transaction::PreviousValue::Any,
        "fixture",
    )
    .unwrap();
}
