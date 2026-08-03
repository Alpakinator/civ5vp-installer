//! The real DLL build, against a real Community-Patch-DLL checkout and the real Toolchain.
//!
//! `#[ignore]`d, never deleted (rule 14): the fast suite proves the orchestration against a
//! fake compiler, and only this proves that the extracted SDK, the pinned clang, the
//! transcribed flags and the project-file parsing together produce `CvGameCore_Expansion2.dll`.
//!
//! ```bash
//! CIV5VP_TOOLCHAIN_CACHE=~/.cache/civ5vp-toolchain \
//! CIV5VP_DLL_SOURCE_ROOT=/path/to/Community-Patch-DLL \
//!   cargo test --release -p civ5vp-toolchain --test real_build -- --ignored --nocapture --test-threads 1
//! ```
//!
//! `CIV5VP_DLL_SOURCE_ROOT` must point at a checkout of the mod (any recent Version — the
//! Upstream Cache from a real `civ5vp-sources` run works). With an empty Toolchain Cache the
//! bootstrap downloads ~2.4 GB first; with a populated one the build starts immediately.
//!
//! The result is compared against the DLL checked into the same checkout at
//! `(1) Community Patch/CvGameCore_Expansion2.dll` — the maintainer-built binary players get
//! from the official installer. Functional equivalence is judged the way ticket 06 asks:
//! same PE machine and DLL bit, identical export list, imported DLLs no wider than the
//! reference's, and a size in the same ballpark. Byte identity is not expected: the
//! reference was built by a different compiler binary at a different optimisation vintage.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;

use civ5vp_core::{
    BuildConfiguration, BuildRequest, FortyThreeCivs, ProgressReporter, ToolchainRunner,
};
use civ5vp_toolchain::BootstrappedToolchain;

use support::pe;

fn cache_root() -> PathBuf {
    match std::env::var_os("CIV5VP_TOOLCHAIN_CACHE") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("toolchain-bootstrap"),
    }
}

fn source_root() -> PathBuf {
    let Some(path) = std::env::var_os("CIV5VP_DLL_SOURCE_ROOT") else {
        panic!(
            "set CIV5VP_DLL_SOURCE_ROOT to a Community-Patch-DLL checkout \
             (see this file's header)"
        );
    };
    let path = PathBuf::from(path);
    assert!(
        path.join("CvGameCoreDLL_Expansion2").is_dir(),
        "{} does not look like a Community-Patch-DLL checkout",
        path.display()
    );
    path
}

fn reporting_progress() -> (ProgressReporter, std::thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let printer = std::thread::spawn(move || {
        for event in receiver {
            println!("[{:?}] {}", event.stage, event.message);
        }
    });
    (ProgressReporter::to_channel(sender), printer)
}

fn build_into_with(output_dir: &PathBuf, forty_three_civs: FortyThreeCivs) -> PathBuf {
    fs::create_dir_all(output_dir).unwrap();
    let output_path = output_dir.join("CvGameCore_Expansion2.dll");
    // A stale DLL must not be able to pass as this run's product — the Core does the same.
    let _ = fs::remove_file(&output_path);

    let runner = BootstrappedToolchain::new(cache_root());
    let request = BuildRequest {
        source_root: source_root(),
        forty_three_civs,
        build_configuration: BuildConfiguration::Release,
        version_label: "real-build-test".to_owned(),
        output_path: output_path.clone(),
    };
    let (progress, printer) = reporting_progress();
    let started = std::time::Instant::now();
    let result = runner.build_dll(&request, &progress);
    drop(progress);
    let _ = printer.join();
    if let Err(error) = result {
        panic!("{}\n  detail: {}", error.message(), error.detail());
    }
    println!("built in {:?}", started.elapsed());
    output_path
}

fn build_into(output_dir: &PathBuf) -> PathBuf {
    build_into_with(output_dir, FortyThreeCivs::Disabled)
}

/// The whole thing: bootstrap (or reuse) the Toolchain, build the Release DLL through the
/// `ToolchainRunner` boundary, and compare it with the checked-in reference DLL.
#[test]
#[ignore = "needs CIV5VP_DLL_SOURCE_ROOT and a (populated or 2.4 GB-downloadable) Toolchain Cache"]
fn the_real_dll_builds_and_matches_the_reference() {
    let output_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-dll-build");
    let output_path = build_into(&output_dir);

    let built = fs::read(&output_path).unwrap();
    let ours = pe::parse(&built).unwrap_or_else(|e| panic!("built DLL does not parse: {e}"));
    println!(
        "built:     {} bytes, machine {:#06x}, {} exports, imports {:?}",
        built.len(),
        ours.machine,
        ours.exports.len(),
        ours.imported_dlls
    );

    assert_eq!(ours.machine, pe::MACHINE_I386, "must target 32-bit x86");
    assert!(ours.is_dll, "must carry the PE DLL characteristic");

    // The export surface is the contract with the game, fixed by CvGameCoreDLL.def.
    let def = fs::read_to_string(source_root().join("CvGameCoreDLL_Expansion2/CvGameCoreDLL.def"))
        .unwrap();
    let declared: BTreeSet<String> = def
        .lines()
        .skip_while(|line| line.trim() != "EXPORTS")
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect();
    assert!(!declared.is_empty(), "the .def file must declare exports");
    assert_eq!(
        ours.exports, declared,
        "the built DLL must export exactly what the .def declares"
    );

    // Compare against the maintainer-built DLL checked into the same Version.
    let reference_path = source_root().join("(1) Community Patch/CvGameCore_Expansion2.dll");
    let reference = fs::read(&reference_path).unwrap_or_else(|e| {
        panic!(
            "no reference DLL at {} ({e}) — is this a full checkout?",
            reference_path.display()
        )
    });
    let theirs = pe::parse(&reference).unwrap_or_else(|e| panic!("reference DLL: {e}"));
    println!(
        "reference: {} bytes, machine {:#06x}, {} exports, imports {:?}",
        reference.len(),
        theirs.machine,
        theirs.exports.len(),
        theirs.imported_dlls
    );

    assert_eq!(ours.machine, theirs.machine);
    // The reference additionally exports VC9 CRT lock plumbing (`std::_Init_locks`) that its
    // compiler chose to re-export — noise beyond the .def contract the game never calls.
    // Anything the reference exports that we do not must be that noise, and we must export
    // nothing the reference does not.
    let ours_extra: Vec<&String> = ours.exports.difference(&theirs.exports).collect();
    assert!(
        ours_extra.is_empty(),
        "the built DLL exports names the reference does not: {ours_extra:?}"
    );
    let theirs_extra: Vec<&String> = theirs
        .exports
        .difference(&ours.exports)
        .filter(|name| !name.contains("_Init_locks"))
        .collect();
    assert!(
        theirs_extra.is_empty(),
        "the reference exports names the built DLL lacks: {theirs_extra:?}"
    );
    // Case-folded: import table spellings vary (`KERNEL32.dll` vs `kernel32.dll`).
    let fold = |set: &BTreeSet<String>| -> BTreeSet<String> {
        set.iter().map(|name| name.to_lowercase()).collect()
    };
    let extra: Vec<String> = fold(&ours.imported_dlls)
        .difference(&fold(&theirs.imported_dlls))
        .cloned()
        .collect();
    assert!(
        extra.is_empty(),
        "the built DLL depends on DLLs the reference does not: {extra:?}"
    );

    // Same ballpark, not same bytes. A stub, a partial link, or a debug build all fall far
    // outside a factor of two of the ~10 MB reference.
    let ratio = built.len() as f64 / reference.len() as f64;
    assert!(
        (0.5..=2.0).contains(&ratio),
        "built {} vs reference {} bytes (ratio {ratio:.2})",
        built.len(),
        reference.len()
    );
}

/// The 43-Civs variant really compiles and links with its define, into its own object
/// directory, leaving the plain Release objects untouched.
#[test]
#[ignore = "needs CIV5VP_DLL_SOURCE_ROOT and a populated Toolchain Cache; ~1 min of compiling"]
fn the_43_civs_variant_builds() {
    let output_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-dll-build");
    let output_path = build_into_with(&output_dir, FortyThreeCivs::Enabled);

    let built = fs::read(&output_path).unwrap();
    let parsed = pe::parse(&built).unwrap_or_else(|e| panic!("43-Civs DLL does not parse: {e}"));
    assert_eq!(parsed.machine, pe::MACHINE_I386);
    assert!(parsed.is_dll);
    assert!(parsed.exports.contains("DllGetGameContext"));
    assert!(
        output_dir.join("objects/release-43civs").is_dir(),
        "the variant keeps its own objects"
    );
}

/// Ticket 06's incremental criterion, against the real compiler: touch one source, and the
/// rebuild recompiles exactly one object and relinks.
///
/// Run after (or without) the test above — the first build populates the object cache either
/// way.
#[test]
#[ignore = "needs CIV5VP_DLL_SOURCE_ROOT and a populated Toolchain Cache"]
fn touching_one_source_rebuilds_incrementally() {
    let output_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-dll-build");
    let first = build_into(&output_dir);
    let first_mtime = fs::metadata(&first).unwrap().modified().unwrap();

    // Snapshot every object's mtime.
    let objects_dir = output_dir.join("objects/release");
    let before = object_mtimes(&objects_dir);
    assert!(!before.is_empty(), "the first build must leave objects");

    // Touch one source.
    let touched = source_root().join("CvGameCoreDLL_Expansion2/CvBarbarians.cpp");
    let contents = fs::read(&touched).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&touched, contents).unwrap();

    let started = std::time::Instant::now();
    let second = build_into(&output_dir);
    println!("incremental rebuild in {:?}", started.elapsed());

    let after = object_mtimes(&objects_dir);
    let recompiled: Vec<&String> = after
        .iter()
        .filter(|(name, mtime)| before.get(*name) != Some(mtime))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        recompiled,
        vec!["CvGameCoreDLL_Expansion2/CvBarbarians.obj"],
        "exactly the touched source's object recompiles"
    );
    assert!(
        fs::metadata(&second).unwrap().modified().unwrap() > first_mtime,
        "the DLL must have been relinked"
    );
}

fn object_mtimes(dir: &PathBuf) -> std::collections::BTreeMap<String, std::time::SystemTime> {
    let mut mtimes = std::collections::BTreeMap::new();
    let mut stack = vec![dir.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "obj") {
                let name = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                mtimes.insert(name, fs::metadata(&path).unwrap().modified().unwrap());
            }
        }
    }
    mtimes
}
