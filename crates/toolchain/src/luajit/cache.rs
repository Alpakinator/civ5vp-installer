//! Engines already built, and how to know one is the engine this build would produce.
//!
//! Nothing about the Lua engine depends on Vox Populi: it is the pinned LuaJIT source, the
//! patches in [`crate::luajit::patches`], and the bootstrapped compiler. A player who installs
//! a new Version gets a byte-identical engine out of a minute of compiling — so the engine is
//! kept under a name made of exactly those inputs, and the minute is spent once.
//!
//! The name is a hash of the source the build reads, so it cannot go stale by accident: edit a
//! patch, bump the pinned commit, or change the compiler, and the name changes with it. The
//! opposite mistake — a cache that is *not* used when it could be — costs a rebuild and
//! nothing else, which is the right way round for something the game loads.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{ToolchainError, io_error};
use crate::luajit::build::{ENGINE_FILE_NAME, LUAJIT_RELVER};

/// Bumped by hand when something outside the hashed inputs changes what a build produces —
/// the invocation plan, say, or a compiler flag.
const CACHE_VERSION: u32 = 1;

/// Files under `src/` that the build writes rather than reads.
///
/// They are derived from the hashed inputs, so hashing them as well would only mean that the
/// first build's own output changed the name the second build looks under — a cache that
/// never hits.
const GENERATED: [&str; 8] = [
    "luajit.h",
    "lj_bcdef.h",
    "lj_ffdef.h",
    "lj_libdef.h",
    "lj_recdef.h",
    "lj_folddef.h",
    "buildvm_arch.h",
    "vmdef.lua",
];

/// Extensions worth hashing: everything the compiler, DynASM or buildvm reads.
const SOURCE_EXTENSIONS: [&str; 4] = ["c", "h", "dasc", "lua"];

/// What this build would produce, named by everything it is made of.
///
/// `source_root` is the LuaJIT checkout — both `src/` and `dynasm/` are inputs, and the
/// patches are already in the files by the time this runs, so a change to one of them shows
/// up here without needing to be listed.
pub fn fingerprint(source_root: &Path, toolchain_identity: &str) -> Result<String, ToolchainError> {
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "civ5vp-luajit-cache v{CACHE_VERSION}\n{toolchain_identity}\n{LUAJIT_RELVER}\n"
    ));

    let mut files = Vec::new();
    for directory in ["src", "dynasm"] {
        collect(&source_root.join(directory), source_root, &mut files)?;
    }
    files.sort();

    for (relative, path) in &files {
        let bytes = std::fs::read(path)
            .map_err(|error| io_error("read the LuaJIT source", path, &error))?;
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes.len().to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex(&hasher.finalize()))
}

/// Every hashable file under `directory`, as `(path relative to the checkout, full path)`.
fn collect(
    directory: &Path,
    source_root: &Path,
    into: &mut Vec<(String, PathBuf)>,
) -> Result<(), ToolchainError> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        // A checkout without one of these is not this function's problem to report: the build
        // itself says so, in a sentence about the source being incomplete.
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, source_root, into)?;
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if GENERATED.contains(&name.as_str()) {
            continue;
        }
        let hashable = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension));
        if !hashable {
            continue;
        }
        let relative = path
            .strip_prefix(source_root)
            .unwrap_or(&path)
            .to_string_lossy()
            // The name has to mean the same thing on both platforms, and one of them writes
            // its separators the other way round.
            .replace('\\', "/");
        into.push((relative, path));
    }
    Ok(())
}

/// The engine already built from these inputs, if there is one.
pub fn look_up(engines_dir: &Path, fingerprint: &str) -> Option<PathBuf> {
    let engine = engines_dir.join(fingerprint).join(ENGINE_FILE_NAME);
    engine.is_file().then_some(engine)
}

/// Keep `built` as the engine for these inputs, and drop the ones it replaces.
///
/// Best-effort by design: a cache that could not be written is a slower next install, not a
/// failed one, so nothing here turns into an error the player sees.
pub fn keep(engines_dir: &Path, fingerprint: &str, built: &Path) -> Option<PathBuf> {
    let directory = engines_dir.join(fingerprint);
    std::fs::create_dir_all(&directory).ok()?;

    // Through a temporary file: a half-copied DLL under the right name is the one thing this
    // cache must never hand back.
    let partial = directory.join(format!("{ENGINE_FILE_NAME}.part"));
    let engine = directory.join(ENGINE_FILE_NAME);
    std::fs::copy(built, &partial).ok()?;
    std::fs::rename(&partial, &engine).ok()?;

    // Everything else here was built from inputs that no longer exist on this machine — a
    // superseded compiler, or a LuaJIT commit that has been bumped.
    if let Ok(entries) = std::fs::read_dir(engines_dir) {
        for entry in entries.flatten() {
            if entry.file_name() != *fingerprint {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Some(engine)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A checkout with the two directories the fingerprint reads.
    fn checkout(dir: &Path) {
        std::fs::create_dir_all(dir.join("src/host")).unwrap();
        std::fs::create_dir_all(dir.join("dynasm")).unwrap();
        std::fs::write(dir.join("src/lib_table.c"), "int table_insert(void);\n").unwrap();
        std::fs::write(dir.join("src/lj_obj.h"), "struct GCtab;\n").unwrap();
        std::fs::write(dir.join("src/vm_x86.dasc"), "|.arch x86\n").unwrap();
        std::fs::write(dir.join("src/host/minilua.c"), "int main(void);\n").unwrap();
        std::fs::write(dir.join("dynasm/dynasm.lua"), "-- dynasm\n").unwrap();
    }

    #[test]
    fn the_same_checkout_and_toolchain_name_the_same_engine() {
        let dir = tempfile::tempdir().unwrap();
        checkout(dir.path());

        let first = fingerprint(dir.path(), "clang-18.1.8").unwrap();
        let second = fingerprint(dir.path(), "clang-18.1.8").unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }

    /// Every input that can change the engine has to change the name — the patches most of
    /// all, since they are edits to a file that is otherwise pinned.
    #[test]
    fn changing_any_input_names_a_different_engine() {
        let dir = tempfile::tempdir().unwrap();
        checkout(dir.path());
        let original = fingerprint(dir.path(), "clang-18.1.8").unwrap();

        assert_ne!(
            fingerprint(dir.path(), "clang-19.0.0").unwrap(),
            original,
            "a different compiler builds a different engine"
        );

        std::fs::write(dir.path().join("src/lib_table.c"), "int patched(void);\n").unwrap();
        assert_ne!(
            fingerprint(dir.path(), "clang-18.1.8").unwrap(),
            original,
            "a patched source builds a different engine"
        );
    }

    /// The first build writes its own generated headers into `src`. If those counted, the
    /// second build would look under a different name and never find anything.
    #[test]
    fn what_the_build_generates_does_not_change_the_name() {
        let dir = tempfile::tempdir().unwrap();
        checkout(dir.path());
        let before = fingerprint(dir.path(), "clang-18.1.8").unwrap();

        for generated in ["lj_bcdef.h", "lj_libdef.h", "luajit.h"] {
            std::fs::write(dir.path().join("src").join(generated), "/* built */\n").unwrap();
        }
        std::fs::write(dir.path().join("src/host/buildvm_arch.h"), "/* built */\n").unwrap();
        std::fs::write(dir.path().join("src/luajit_relver.txt"), "1785763465").unwrap();
        std::fs::write(dir.path().join("src/lj_vm.obj"), [0u8; 8]).unwrap();

        assert_eq!(fingerprint(dir.path(), "clang-18.1.8").unwrap(), before);
    }

    #[test]
    fn an_engine_is_kept_and_found_again() {
        let dir = tempfile::tempdir().unwrap();
        let engines = dir.path().join("engines");
        let built = dir.path().join(ENGINE_FILE_NAME);
        std::fs::write(&built, b"MZ engine").unwrap();

        assert!(look_up(&engines, "abc").is_none());
        let kept = keep(&engines, "abc", &built).unwrap();

        assert_eq!(look_up(&engines, "abc"), Some(kept));
        assert_eq!(
            std::fs::read(engines.join("abc").join(ENGINE_FILE_NAME)).unwrap(),
            b"MZ engine"
        );
        assert!(
            !engines
                .join("abc")
                .join(format!("{ENGINE_FILE_NAME}.part"))
                .exists(),
            "the temporary copy is not left behind"
        );
    }

    /// One engine is worth keeping: the one this installer would build now. The rest were
    /// built from a compiler or a LuaJIT commit that is no longer on this machine.
    #[test]
    fn keeping_an_engine_drops_the_ones_it_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let engines = dir.path().join("engines");
        let built = dir.path().join(ENGINE_FILE_NAME);
        std::fs::write(&built, b"MZ engine").unwrap();
        keep(&engines, "older", &built).unwrap();

        keep(&engines, "newer", &built).unwrap();

        assert!(look_up(&engines, "newer").is_some());
        assert!(look_up(&engines, "older").is_none());
        assert!(!engines.join("older").exists());
    }
}
