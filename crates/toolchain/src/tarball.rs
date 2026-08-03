//! Unpacking the portable LLVM `.tar.xz`, in process and without a temporary copy of the
//! 4.5 GB the archive expands to.
//!
//! xz decompression is push-shaped (`lzma-rs` writes into a sink) while `tar` is pull-shaped
//! (it reads). A `std::io::pipe` and one worker thread join them, so the archive streams
//! straight from the download into the extraction filter and only the members that are kept
//! ever touch the disk.

use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};

use crate::error::{ToolchainError, io_error, stream_error};

// `PathBuf` is used by `safe_join` and `clang_path`.

/// The tools in `bin/` a `clang-cl` → `lld-link` build of the DLL needs, by exact name.
///
/// An allowlist rather than "keep `bin/`", because these binaries each statically link their
/// half of LLVM: `bin/` unpacks to **4.9 GB** while this set is **338 MB**. Keeping the lot
/// would put the Toolchain Cache past the ~5 GB the spec surfaces for the *whole* App Data
/// Store, on its own.
///
/// Symlinks count as entries, so each tool's real binary is listed next to the names that
/// reach it: `clang-cl` → `clang` → `clang-18`, `lld-link` → `lld`, `llvm-lib` → `llvm-ar`.
/// Ticket 06 owns the build and may find it needs another tool; adding one here is a line.
const KEPT_TOOLS: &[&str] = &[
    // The compiler driver, under every name it is invoked by.
    "clang",
    "clang++",
    "clang-18",
    "clang-cl",
    "clang-cpp",
    // The linker.
    "ld.lld",
    "lld",
    "lld-link",
    // Static/import library handling, which `llvm-lib` fronts.
    "llvm-ar",
    "llvm-dlltool",
    "llvm-lib",
    "llvm-ranlib",
    // Resource compilation and manifests, for the DLL's `.rc`.
    "llvm-mt",
    "llvm-rc",
    "llvm-windres",
];

/// Whether one archive member belongs in the Toolchain Cache.
///
/// Three things do: a tool from [`KEPT_TOOLS`], the compiler's builtin headers under
/// `lib/clang/`, and the shared libraries directly in `lib/` that those tools load at runtime.
/// Everything else — LLVM's own static libraries, its headers, cmake files, docs, and the
/// three quarters of `bin/` that is lldb, mlir, flang and the analysis tools — is dropped as
/// it goes past.
fn is_kept(relative: &str) -> bool {
    if relative.starts_with("lib/clang/") {
        return true;
    }
    if let Some(tool) = relative.strip_prefix("bin/") {
        // Windows spells the same tools with `.exe`.
        let tool = tool.strip_suffix(".exe").unwrap_or(tool);
        return !tool.contains('/') && KEPT_TOOLS.contains(&tool);
    }
    match relative.strip_prefix("lib/") {
        // Only files directly in `lib/`, and only dynamic libraries.
        Some(rest) if !rest.contains('/') => {
            rest.contains(".so") || rest.ends_with(".dll") || rest.ends_with(".dylib")
        }
        _ => false,
    }
}

/// Unpack `archive` into `destination`, stripping `archive_root` and keeping only what
/// [`is_kept`] allows.
pub fn extract_llvm(
    archive: &Path,
    archive_root: &str,
    destination: &Path,
    progress: &ProgressReporter,
) -> Result<usize, ToolchainError> {
    fs::create_dir_all(destination)
        .map_err(|error| io_error("create the compiler folder", destination, &error))?;

    let file = fs::File::open(archive)
        .map_err(|error| io_error("open the compiler archive", archive, &error))?;
    let (pipe_reader, mut pipe_writer) =
        std::io::pipe().map_err(|error| stream_error("open a decompression pipe", &error))?;

    // The decompressor runs on its own thread and pushes into the pipe; this thread pulls the
    // tar entries out of the other end.
    let decoder = std::thread::spawn(move || {
        let mut input = BufReader::new(file);
        let result = lzma_rs::xz_decompress(&mut input, &mut pipe_writer);
        // Dropping the writer is what tells the reader the archive ended; do it before the
        // thread's result is collected so a decode failure cannot deadlock the reader.
        let _ = pipe_writer.flush();
        drop(pipe_writer);
        result
    });

    let mut archive_reader = tar::Archive::new(pipe_reader);
    let mut kept = 0usize;
    let mut seen = 0usize;
    let entries = archive_reader
        .entries()
        .map_err(|error| stream_error("read the compiler archive", &error))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| stream_error("read the compiler archive", &error))?;
        let path = entry
            .path()
            .map_err(|error| stream_error("read a name from the compiler archive", &error))?
            .to_path_buf();
        seen += 1;
        if seen.is_multiple_of(2_000) {
            progress.report(
                Stage::Build,
                format!("Unpacking the compiler — {kept} files kept."),
            );
        }

        let Some(relative) = strip_root(&path, archive_root) else {
            continue;
        };
        if !is_kept(&relative) {
            continue;
        }
        // The entry's own path still carries the archive root, so the destination is built
        // here rather than left to `unpack_in`. `safe_join` is what keeps an archive that
        // arrived over the network from writing outside `destination`.
        let Some(target) = safe_join(destination, &relative) else {
            return Err(ToolchainError::new(
                "The compiler download is not the file the installer expected.",
                format!("refusing to unpack {relative}: unsafe path"),
            ));
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("create a compiler folder", parent, &error))?;
        }
        entry
            .unpack(&target)
            .map_err(|error| stream_error("unpack the compiler archive", &error))?;
        kept += 1;
    }

    match decoder.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            return Err(ToolchainError::new(
                "The compiler download came out damaged. Clear the installer's data folder \
                 and try again.",
                format!("xz decompression of {} failed: {error}", archive.display()),
            ));
        }
        Err(_) => {
            return Err(ToolchainError::new(
                "The installer could not unpack the compiler.",
                "the decompression thread stopped unexpectedly".to_string(),
            ));
        }
    }

    if kept == 0 {
        return Err(ToolchainError::new(
            "The compiler download is not the file the installer expected.",
            format!("no members under {archive_root}/ in {}", archive.display()),
        ));
    }
    Ok(kept)
}

/// `clang+llvm-.../bin/clang` → `bin/clang`; anything outside the expected root → `None`.
fn strip_root(path: &Path, archive_root: &str) -> Option<String> {
    let text = path.to_str()?;
    let rest = text.strip_prefix(archive_root)?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Append a `/`-separated relative path to `root`, refusing anything that is not a plain
/// sequence of ordinary names.
fn safe_join(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    let mut pushed = 0;
    for segment in relative.split('/').filter(|s| !s.is_empty() && *s != ".") {
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(std::path::Component::Normal(name)), None) => out.push(name),
            _ => return None,
        }
        pushed += 1;
    }
    (pushed > 0).then_some(out)
}

/// Where the bootstrapped `clang` ends up under the extraction destination.
pub fn clang_path(llvm_root: &Path) -> PathBuf {
    llvm_root.join("bin").join(if cfg!(windows) {
        "clang-cl.exe"
    } else {
        "clang-cl"
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Unpack a real LLVM release tarball, without the 1.45 GiB disc image alongside it.
    ///
    /// `#[ignore]`d and driven by an environment variable. It exists because the fixture in
    /// this file is a few kilobytes compressed by `lzma-rs` itself, and the pinned artifact is
    /// a gigabyte compressed by whatever `xz` llvm.org runs — a stream with many blocks, and
    /// the one place a pure-Rust xz decoder is most likely to disagree with the encoder that
    /// produced it.
    ///
    /// ```bash
    /// CIV5VP_LLVM_ARCHIVE=/path/to/clang+llvm-18.1.8-....tar.xz \
    ///   cargo test --release -p civ5vp-toolchain --lib -- --ignored --nocapture unpacks_a_real
    /// ```
    #[test]
    #[ignore = "needs a real LLVM release tarball in CIV5VP_LLVM_ARCHIVE"]
    fn unpacks_a_real_llvm_release() {
        let Some(archive) = std::env::var_os("CIV5VP_LLVM_ARCHIVE") else {
            panic!("set CIV5VP_LLVM_ARCHIVE to a clang+llvm release tarball");
        };
        let archive = PathBuf::from(archive);
        let Some(pinned) = crate::pinned::llvm_for_host() else {
            panic!("no pinned LLVM for this host to compare against");
        };

        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let kept = extract_llvm(
            &archive,
            pinned.archive_root,
            dir.path(),
            &civ5vp_core::ProgressReporter::silent(),
        )
        .unwrap_or_else(|error| panic!("{}\n  detail: {}", error.message(), error.detail()));

        println!("kept {kept} files in {:?}", started.elapsed());
        let bytes: u64 = walkdir_size(dir.path());
        println!("unpacked {:.0} MB", bytes as f64 / (1024.0 * 1024.0));
        let clang = clang_path(dir.path());
        assert!(clang.is_file(), "{} should exist", clang.display());
        assert!(dir.path().join("bin/lld-link").exists());
        assert!(dir.path().join("lib/clang/18/include/stddef.h").exists());
        // The filter did its job: none of LLVM's own static libraries came through.
        assert!(!dir.path().join("lib/libLLVMCore.a").exists());
    }

    /// Total size of the real files under `root`, following nothing.
    fn walkdir_size(root: &Path) -> u64 {
        let mut total = 0;
        let mut queue = vec![root.to_path_buf()];
        while let Some(directory) = queue.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if metadata.is_dir() {
                    queue.push(path);
                } else if metadata.is_file() {
                    total += metadata.len();
                }
            }
        }
        total
    }

    #[test]
    fn only_the_compiler_driver_linker_and_builtin_headers_are_kept() {
        // The driver and everything the symlink chain to it passes through.
        assert!(is_kept("bin/clang-cl"));
        assert!(is_kept("bin/clang"));
        assert!(is_kept("bin/clang-18"));
        assert!(is_kept("bin/lld-link"));
        assert!(is_kept("bin/lld"));
        assert!(is_kept("bin/llvm-lib"));
        assert!(is_kept("bin/llvm-ar"));
        assert!(is_kept("bin/clang-cl.exe"));
        assert!(is_kept("lib/clang/18/include/stddef.h"));
        assert!(is_kept("lib/libclang-cpp.so.18.1"));
        assert!(is_kept("lib/LLVM-C.dll"));

        // The rest of `bin/` is 4.6 of its 4.9 GB, and none of it compiles anything here.
        assert!(!is_kept("bin/clang-tidy"));
        assert!(!is_kept("bin/clang-scan-deps"));
        assert!(!is_kept("bin/lldb"));
        assert!(!is_kept("bin/flang-new"));
        assert!(!is_kept("bin/mlir-opt"));
        assert!(!is_kept("bin/opt"));

        // Nor are LLVM's static libraries, its own headers, or its build system.
        assert!(!is_kept("lib/libLLVMCore.a"));
        assert!(!is_kept("include/llvm/ADT/APInt.h"));
        assert!(!is_kept("lib/cmake/llvm/LLVMConfig.cmake"));
        assert!(!is_kept("share/man/man1/clang.1"));
    }

    /// Every name the build reaches the driver and linker by has to resolve, which means the
    /// targets of those symlinks are in the list too.
    #[test]
    fn the_tool_list_is_closed_under_the_symlinks_that_reach_it() {
        for (name, target) in [
            ("clang-cl", "clang"),
            ("clang", "clang-18"),
            ("lld-link", "lld"),
            ("llvm-lib", "llvm-ar"),
            ("llvm-windres", "llvm-rc"),
        ] {
            assert!(KEPT_TOOLS.contains(&name), "{name}");
            assert!(
                KEPT_TOOLS.contains(&target),
                "{target}, the target of {name}"
            );
        }
    }

    #[test]
    fn the_archives_single_root_directory_is_stripped() {
        assert_eq!(
            strip_root(
                Path::new("clang+llvm-18.1.8-x/bin/clang"),
                "clang+llvm-18.1.8-x"
            ),
            Some("bin/clang".to_string())
        );
        // The root itself contributes nothing.
        assert_eq!(
            strip_root(Path::new("clang+llvm-18.1.8-x/"), "clang+llvm-18.1.8-x"),
            None
        );
        // A member from some other tree is not ours to unpack.
        assert_eq!(
            strip_root(Path::new("elsewhere/bin/clang"), "clang+llvm-18.1.8-x"),
            None
        );
    }

    /// A real round trip: build a tar, xz it with the same pure-Rust stack the bootstrap
    /// reads with, and check the filter and the strip both took effect.
    #[test]
    fn a_tar_xz_round_trips_through_the_pipe_and_the_filter() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("llvm.tar.xz");
        fs::write(&archive, sample_tar_xz("llvm-root")).unwrap();

        let destination = dir.path().join("out");
        let kept = extract_llvm(
            &archive,
            "llvm-root",
            &destination,
            &ProgressReporter::silent(),
        )
        .unwrap();

        assert_eq!(kept, 3);
        assert_eq!(
            fs::read_to_string(destination.join("bin/clang-cl")).unwrap(),
            "driver"
        );
        assert_eq!(
            fs::read_to_string(destination.join("lib/clang/18/include/stddef.h")).unwrap(),
            "builtin"
        );
        assert!(destination.join("lib/libclang-cpp.so.18.1").exists());
        assert!(!destination.join("lib/libLLVMCore.a").exists());
        assert!(!destination.join("include").exists());
    }

    #[test]
    fn an_archive_with_nothing_under_the_expected_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("llvm.tar.xz");
        fs::write(&archive, sample_tar_xz("some-other-root")).unwrap();

        let error = extract_llvm(
            &archive,
            "llvm-root",
            &dir.path().join("out"),
            &ProgressReporter::silent(),
        )
        .unwrap_err();

        assert!(error.detail().contains("no members under llvm-root/"));
    }

    /// `lzma-rs` only decodes, so the fixture is compressed with its LZMA2/xz *encoder*
    /// counterpart — `lzma_rs::xz_compress`, which the crate also ships.
    fn sample_tar_xz(root: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, content) in [
            ("bin/clang-cl", "driver"),
            ("lib/clang/18/include/stddef.h", "builtin"),
            ("lib/libclang-cpp.so.18.1", "shared"),
            ("lib/libLLVMCore.a", "static"),
            ("include/llvm/ADT/APInt.h", "header"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("{root}/{name}"), content.as_bytes())
                .unwrap();
        }
        let tar_bytes = builder.into_inner().unwrap();

        let mut compressed = Vec::new();
        lzma_rs::xz_compress(&mut tar_bytes.as_slice(), &mut compressed).unwrap();
        compressed
    }
}
