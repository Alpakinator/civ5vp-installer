//! The real Toolchain Bootstrap, against the artifacts `docs/pinned-artifacts.md` pins.
//!
//! `#[ignore]`d, never deleted (rule 14). It downloads ~1.6 GB and unpacks tens of thousands
//! of files, so it is nothing like a per-commit test — but a bootstrap that has never once
//! extracted the real ISO has not resolved the spec's extraction-fidelity bet, and that bet
//! is what ticket 05 exists to settle.
//!
//! ```bash
//! cargo test -p civ5vp-toolchain -- --ignored --nocapture
//! ```
//!
//! Set `CIV5VP_TOOLCHAIN_CACHE` to a directory you want to keep, or the downloads land in
//! `target/tmp` and survive only until the next `cargo clean`. The bootstrap resumes and
//! reuses verified downloads, so a re-run after an interrupted one is cheap.

use std::path::PathBuf;
use std::sync::mpsc;

use civ5vp_toolchain::{REFERENCE_BASELINE, ToolchainBootstrap, verify_extraction};

/// Where the real Toolchain Cache goes for these tests. Deliberately persistent: nobody
/// should pay 1.6 GB twice to re-run one assertion.
fn cache_root() -> PathBuf {
    match std::env::var_os("CIV5VP_TOOLCHAIN_CACHE") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("toolchain-bootstrap"),
    }
}

/// Print progress as it arrives, on a thread, so a 40-minute download is watchable.
fn reporting_progress() -> (civ5vp_core::ProgressReporter, std::thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel::<civ5vp_core::ProgressEvent>();
    let printer = std::thread::spawn(move || {
        for event in receiver {
            println!("[{:?}] {}", event.stage, event.message);
        }
    });
    (civ5vp_core::ProgressReporter::to_channel(sender), printer)
}

/// The whole thing: download both pinned artifacts, verify their checksums, extract the four
/// ISO members through MSI and CAB, apply the six Linux fix-ups, and check that every name in
/// `docs/pinned-artifacts.md` §4 resolves.
#[test]
#[ignore = "downloads ~1.6 GB from archive.org and github.com"]
fn the_real_sdk_iso_extracts_into_a_usable_toolchain() {
    let root = cache_root();
    println!("Toolchain Cache: {}", root.display());

    let (progress, printer) = reporting_progress();
    let bootstrap = ToolchainBootstrap::new(root.clone());
    let toolchain = match bootstrap.ensure(&progress) {
        Ok(toolchain) => toolchain,
        Err(error) => panic!("{}\n  detail: {}", error.message(), error.detail()),
    };
    drop(progress);
    let _ = printer.join();

    let report = verify_extraction(toolchain.sdk_root()).unwrap();
    println!("\n=== measured extraction ===");
    println!("identity: {}", toolchain.identity());
    println!("headers:  {}", report.headers);
    println!("libs:     {}", report.libs);
    for (name, path) in &report.resolved {
        println!("  {name} -> {}", path.display());
    }
    assert!(
        report.missing.is_empty(),
        "these did not resolve: {:?}",
        report.missing
    );

    // The compiler half.
    let clang = toolchain.clang_path();
    assert!(clang.is_file(), "{} should exist", clang.display());

    // What ticket 06 will need: the MSIs bury these, so print where they actually landed.
    let include_dirs = toolchain.include_dirs().unwrap();
    let lib_dirs = toolchain.lib_dirs().unwrap();
    println!("include dirs:");
    for dir in &include_dirs {
        println!("  {}", dir.display());
    }
    println!("lib dirs:");
    for dir in &lib_dirs {
        println!("  {}", dir.display());
    }
    assert!(!include_dirs.is_empty(), "the SDK must contribute headers");
    assert!(!lib_dirs.is_empty(), "the SDK must contribute libraries");

    // The counts are a regression guard, not a claim about the docker image — see the
    // constant's own documentation.
    if let Some(baseline) = REFERENCE_BASELINE {
        assert_eq!(
            (report.headers, report.libs),
            (baseline.headers, baseline.libs),
            "extraction no longer matches the committed baseline"
        );
    } else {
        println!(
            "\nno committed baseline yet; measured headers={} libs={}",
            report.headers, report.libs
        );
    }
}

/// The second half of "bootstrap runs once": with the cache already populated, `ensure` must
/// return immediately and touch nothing.
///
/// Runs after the test above has populated the cache; on its own it will do the full
/// bootstrap first, which is the same thing from a colder start.
#[test]
#[ignore = "needs a populated Toolchain Cache from the bootstrap test"]
fn a_populated_toolchain_cache_is_reused_instantly() {
    let root = cache_root();
    let bootstrap = ToolchainBootstrap::new(root);

    let started = std::time::Instant::now();
    let first = match bootstrap.ensure(&civ5vp_core::ProgressReporter::silent()) {
        Ok(toolchain) => toolchain,
        Err(error) => panic!("{}\n  detail: {}", error.message(), error.detail()),
    };
    let elapsed = started.elapsed();

    let second = bootstrap
        .ensure(&civ5vp_core::ProgressReporter::silent())
        .unwrap();

    assert_eq!(first.identity(), second.identity());
    assert_eq!(first.sdk_root(), second.sdk_root());
    println!("cache hit in {elapsed:?}");
}
