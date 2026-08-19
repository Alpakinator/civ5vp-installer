//! A dry run of a real Vox Populi merge, on a machine that has real data.
//!
//! Ignored in the fast suite - multi-hundred-megabyte inputs, minutes of work. Run by hand
//! while developing:
//!
//! ```text
//! CIV5VP_REAL_MERGE_LIST=/path/to/updates.txt \
//! CIV5VP_REAL_MERGE_BASE=~/.local/share/civ5vp-installer/modpack-base \
//! CIV5VP_REAL_MERGE_OUT=/tmp/real-merge \
//! cargo test -p civ5vp-modpack --test real_merge -- --ignored --nocapture
//! ```
//!
//! `LIST` is the ordered update files, one absolute path per line - what
//! `core::modpack::collect_database_updates` would produce.

use std::path::PathBuf;

use civ5vp_core::{ModpackAssembler, ModpackDatabaseJob, ProgressReporter};
use civ5vp_modpack::SqliteModpackAssembler;

#[test]
#[ignore = "needs real VP data; see the module docs"]
fn the_real_vox_populi_merge_runs_dry() {
    let list = std::env::var("CIV5VP_REAL_MERGE_LIST").expect("CIV5VP_REAL_MERGE_LIST");
    let base = PathBuf::from(std::env::var("CIV5VP_REAL_MERGE_BASE").expect("_BASE"));
    let out = PathBuf::from(std::env::var("CIV5VP_REAL_MERGE_OUT").expect("_OUT"));
    std::fs::create_dir_all(&out).unwrap();

    let updates: Vec<PathBuf> = std::fs::read_to_string(&list)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    println!("merging {} update files", updates.len());

    let job = ModpackDatabaseJob {
        gameplay_base: base.join("Civ5DebugDatabase.db"),
        text_base: base.join("Localization-Merged.db"),
        updates,
        gameplay_dump: out.join("CIV5Units.xml"),
        text_dump: out.join("CIV5Units_Mongol.xml"),
        scratch_dir: out.join("scratch"),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let started = std::time::Instant::now();
    let result =
        SqliteModpackAssembler::new().merge_and_dump(&job, &ProgressReporter::to_channel(sender));
    for event in receiver.try_iter() {
        // Applying-lines are routine; anything else is worth eyes.
        if !event.message.starts_with("Applying ") {
            println!("note: {}", event.message);
        }
    }
    if let Err(error) = &result {
        panic!("merge failed: {} - {}", error.message(), error.detail());
    }
    println!(
        "merged in {:?}; gameplay dump {} MB, text dump {} MB",
        started.elapsed(),
        std::fs::metadata(&job.gameplay_dump).unwrap().len() / 1_048_576,
        std::fs::metadata(&job.text_dump).unwrap().len() / 1_048_576,
    );
}
