//! The six post-extraction fix-ups from `docs/pinned-artifacts.md` §3.
//!
//! The Windows SDK was authored for a case-insensitive filesystem that treats `\` as a path
//! separator, and it is genuinely inconsistent about both: `windows.h` includes `WinDef.h`,
//! `winioctl.h` includes `pshpack1.h` through a backslashed path, `Kernel32.Lib` and
//! `kernel32.lib` both appear in the same tree. On Linux none of that resolves, and the fix
//! is not to patch the compiler invocation but to make the extracted tree answer to every
//! spelling that appears in it.
//!
//! On Windows the whole set is a no-op: the filesystem already does all six.
//!
//! Everything here walks directories in sorted order and produces the same tree from the same
//! input, because the Build Fingerprint is taken over what this leaves behind (rule 8).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};

use crate::error::{ToolchainError, io_error};
use crate::pinned::WDK_STUB_HEADERS;
use crate::sdk_layout;

/// What the fix-ups changed. Reported to the log, and what the tests assert on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixupReport {
    /// Fix-up 1: files under `Include/` renamed to lowercase.
    pub lowercased: usize,
    /// Fix-up 2: symlinks added for `#include` names that differ only in case.
    pub include_case_links: usize,
    /// Fix-up 3: `Include`/`Lib` directory spellings made to resolve.
    pub directory_links: usize,
    /// Fix-up 4: headers rewritten to use `/` in `#include` directives.
    pub backslashes_rewritten: usize,
    /// Fix-up 5: extra spellings added for `.lib` files.
    pub lib_case_links: usize,
    /// Fix-up 6: WDK-only headers stubbed out.
    pub wdk_stubs: usize,
}

/// Apply all six, in the only order they work in.
///
/// Backslashes go first so the case-mismatch pass sees real path segments; lowercasing and
/// the WDK stubs go before it so it is matching against the final tree.
pub fn apply(sdk_root: &Path, progress: &ProgressReporter) -> Result<FixupReport, ToolchainError> {
    let mut report = FixupReport::default();
    if !platform::NEEDS_FIXUPS {
        // Documented rather than silent: on Windows the filesystem is the fix-up.
        return Ok(report);
    }

    // Where `Include` and `Lib` are is a question, not an assumption: the MSIs bury them
    // several levels down, under the path Windows would have installed to. See `sdk_layout`.
    let roots = sdk_layout::find(sdk_root)?;

    progress.report(Stage::Build, "Adjusting the SDK for Linux.");

    for root in &roots.include {
        report.backslashes_rewritten += rewrite_backslash_includes(root)?;
    }
    for root in &roots.include {
        report.lowercased += lowercase_file_names(root)?;
    }
    for root in &roots.include {
        report.wdk_stubs += stub_wdk_headers(root)?;
    }
    for root in &roots.include {
        report.include_case_links += link_case_mismatched_includes(root)?;
    }
    for root in roots.include.iter().chain(roots.lib.iter()) {
        report.directory_links += link_directory_spellings(root)?;
    }
    for root in &roots.lib {
        report.lib_case_links += link_lib_spellings(root)?;
    }

    progress.report(
        Stage::Build,
        format!(
            "SDK adjusted: {} headers renamed, {} link names added.",
            report.lowercased,
            report.include_case_links + report.lib_case_links + report.directory_links
        ),
    );
    Ok(report)
}

/// **Fix-up 1** — lowercase every filename under `Include/`, leaving a symlink from the
/// original mixed-case name to the lowercase one.
///
/// Both directions are needed: the build's own sources include `<windows.h>` while the SDK's
/// headers include `<WinDef.h>`, and only one of those can be the real file.
fn lowercase_file_names(root: &Path) -> Result<usize, ToolchainError> {
    let mut renamed = 0;
    for file in walk_files(root)? {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lowered = name.to_ascii_lowercase();
        if lowered == name {
            continue;
        }
        let Some(parent) = file.parent() else {
            continue;
        };
        let target = parent.join(&lowered);
        if target.exists() {
            // Both spellings already present as real files — nothing to rename, and the
            // mixed-case one must not be clobbered.
            continue;
        }
        fs::rename(&file, &target)
            .map_err(|error| io_error("rename a header to lowercase", &file, &error))?;
        platform::symlink(Path::new(&lowered), &file)
            .map_err(|error| io_error("link a header's original name", &file, &error))?;
        renamed += 1;
    }
    Ok(renamed)
}

/// **Fix-up 4** — rewrite backslashes to forward slashes inside `#include` directives.
///
/// Only inside the directive's delimiters: a backslash anywhere else in a header is a line
/// continuation or a string escape and rewriting it would change the code.
fn rewrite_backslash_includes(root: &Path) -> Result<usize, ToolchainError> {
    let mut rewritten = 0;
    for file in walk_files(root)? {
        let Ok(text) = fs::read_to_string(&file) else {
            // Not UTF-8 — not a header. The SDK ships a few binary blobs under Include/.
            continue;
        };
        let updated = rewrite_include_lines(&text);
        if updated != text {
            fs::write(&file, updated)
                .map_err(|error| io_error("rewrite a header's includes", &file, &error))?;
            rewritten += 1;
        }
    }
    Ok(rewritten)
}

/// The line-level half of fix-up 4, split out so it can be tested without a filesystem.
fn rewrite_include_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        match include_target_span(line) {
            Some((start, end)) if line[start..end].contains('\\') => {
                out.push_str(&line[..start]);
                out.push_str(&line[start..end].replace('\\', "/"));
                out.push_str(&line[end..]);
            }
            _ => out.push_str(line),
        }
    }
    out
}

/// Byte range of the name inside `#include <...>` or `#include "..."`, if this line is one.
fn include_target_span(line: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let rest = trimmed.strip_prefix('#')?.trim_start();
    // `#` plus any whitespace after it — `#  include` is legal.
    let hash_and_gap = trimmed.len() - rest.len();
    let rest = rest.strip_prefix("include")?;
    let after_keyword = indent + hash_and_gap + "include".len();

    let open = rest.find(['<', '"'])?;
    // Anything between `include` and the delimiter must be whitespace, or this is
    // `#include_next`, or a macro-expanded include we must not touch.
    if !rest[..open].chars().all(char::is_whitespace) {
        return None;
    }
    let closing = match rest.as_bytes().get(open)? {
        b'<' => '>',
        _ => '"',
    };
    let close = rest[open + 1..].find(closing)?;
    let start = after_keyword + open + 1;
    Some((start, start + close))
}

/// **Fix-up 6** — stub the WDK-only headers.
///
/// `windows.h` reaches `DriverSpecs.h` through `specstrings.h`, but that header ships with
/// the Driver Kit, not the SDK. Empty files satisfy the `#include` chain: everything in them
/// is SAL annotation macros that only matter to the driver verifier.
fn stub_wdk_headers(root: &Path) -> Result<usize, ToolchainError> {
    let mut created = 0;
    for name in WDK_STUB_HEADERS {
        let path = root.join(name);
        if path.exists() {
            continue;
        }
        fs::write(
            &path,
            b"/* Stubbed by the Civ 5 VP Installer: WDK-only header. */\n",
        )
        .map_err(|error| io_error("write a stub header", &path, &error))?;
        created += 1;
    }
    Ok(created)
}

/// **Fix-up 2** — where a header includes a name that differs only in case from a file on
/// disk, add a symlink under the spelling the header used.
///
/// Multi-segment names (`GL/gl.h`, `sys\types.h` before fix-up 4 gets to it) are resolved a
/// segment at a time, so a directory whose case is wrong is linked too.
fn link_case_mismatched_includes(root: &Path) -> Result<usize, ToolchainError> {
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for file in walk_files(root)? {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            if let Some((start, end)) = include_target_span(line) {
                wanted.insert(line[start..end].replace('\\', "/"));
            }
        }
    }

    let mut created = 0;
    for name in wanted {
        created += link_path_spelling(root, &name)?;
    }
    Ok(created)
}

/// Make `relative` resolve under `root`, adding a symlink for any segment that exists only
/// under a different case. Returns how many links it had to add.
fn link_path_spelling(root: &Path, relative: &str) -> Result<usize, ToolchainError> {
    let segments: Vec<&str> = relative
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if segments.is_empty() || segments.contains(&"..") {
        return Ok(0);
    }

    let mut created = 0;
    let mut current = root.to_path_buf();
    for segment in segments {
        let candidate = current.join(segment);
        if candidate.exists() {
            current = candidate;
            continue;
        }
        let Some(actual) = case_insensitive_match(&current, segment)? else {
            // The header includes something the SDK does not ship at all. Not this fix-up's
            // problem — fix-up 6 stubs the known ones and the compiler reports the rest.
            return Ok(created);
        };
        platform::symlink(Path::new(&actual), &candidate)
            .map_err(|error| io_error("link an include name", &candidate, &error))?;
        created += 1;
        current = candidate;
    }
    Ok(created)
}

/// **Fix-up 3** — make every spelling of an `Include` or `Lib` directory resolve.
///
/// The build asks for the capitalised names; the SDK MSI ships `Include`/`Lib` while the CRT
/// MSI ships `include`/`lib`, so which spelling exists depends on which half of the image
/// produced it. `root` is a directory `sdk_layout` found; this puts the other spellings beside
/// it.
fn link_directory_spellings(root: &Path) -> Result<usize, ToolchainError> {
    let (Some(parent), Some(name)) = (root.parent(), root.file_name().and_then(|n| n.to_str()))
    else {
        return Ok(0);
    };

    let mut created = 0;
    // A fixed, sorted set, so two runs produce the same tree (rule 8).
    let spellings = BTreeSet::from([name.to_ascii_lowercase(), capitalise(name)]);
    for spelling in spellings {
        if spelling == name {
            continue;
        }
        let link = parent.join(&spelling);
        if link.exists() {
            continue;
        }
        platform::symlink(Path::new(name), &link)
            .map_err(|error| io_error("link a toolchain folder", &link, &error))?;
        created += 1;
    }
    Ok(created)
}

/// `include` → `Include`. ASCII only, which is all these names ever are.
fn capitalise(name: &str) -> String {
    let mut lowered = name.to_ascii_lowercase();
    if let Some(first) = lowered.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    lowered
}

/// **Fix-up 5** — a case symlink for every `.lib`.
///
/// The SDK ships `Kernel32.Lib`, the CRT ships `msvcrt.lib`, and the project file references
/// both spellings and others besides. The set of extra spellings is fixed and sorted so two
/// runs produce the same tree (rule 8).
fn link_lib_spellings(root: &Path) -> Result<usize, ToolchainError> {
    let mut created = 0;
    for file in walk_files(root)? {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(stem) = strip_lib_extension(name) else {
            continue;
        };
        let Some(parent) = file.parent() else {
            continue;
        };

        for spelling in lib_spellings(stem) {
            if spelling == name {
                continue;
            }
            let link = parent.join(&spelling);
            if link.exists() {
                continue;
            }
            platform::symlink(Path::new(name), &link)
                .map_err(|error| io_error("link an import library", &link, &error))?;
            created += 1;
        }
    }
    Ok(created)
}

/// The stem of `X.lib` in any case, or `None` if this is not an import library.
fn strip_lib_extension(name: &str) -> Option<&str> {
    let (stem, extension) = name.rsplit_once('.')?;
    extension.eq_ignore_ascii_case("lib").then_some(stem)
}

/// Every spelling a linker might ask for, sorted and deduplicated.
fn lib_spellings(stem: &str) -> Vec<String> {
    let lower = stem.to_ascii_lowercase();
    let upper = stem.to_ascii_uppercase();
    let mut spellings: BTreeSet<String> = BTreeSet::new();
    for base in [&lower, &upper, &stem.to_string()] {
        for extension in ["lib", "Lib", "LIB"] {
            spellings.insert(format!("{base}.{extension}"));
        }
    }
    spellings.into_iter().collect()
}

/// The single entry in `directory` whose name matches `name` ignoring case, or `None`.
///
/// `None` when several match, too: picking one of `Foo.h` and `FOO.h` arbitrarily would make
/// the extraction non-deterministic, and the ambiguity is better left to the compiler's error
/// message than resolved by a coin flip.
fn case_insensitive_match(directory: &Path, name: &str) -> Result<Option<String>, ToolchainError> {
    let mut found: Option<String> = None;
    let entries = fs::read_dir(directory)
        .map_err(|error| io_error("list a toolchain folder", directory, &error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| io_error("list a toolchain folder", directory, &error))?;
        let entry_name = entry.file_name();
        let Some(entry_name) = entry_name.to_str() else {
            continue;
        };
        if entry_name.eq_ignore_ascii_case(name) {
            if found.is_some() {
                return Ok(None);
            }
            found = Some(entry_name.to_string());
        }
    }
    Ok(found)
}

/// Every regular file under `root`, deepest-last, in sorted order. Symlinks are not followed
/// and are not returned — otherwise each pass would start operating on the previous pass's
/// output.
fn walk_files(root: &Path) -> Result<Vec<PathBuf>, ToolchainError> {
    let mut files = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let mut children: Vec<PathBuf> = fs::read_dir(&directory)
            .map_err(|error| io_error("list a toolchain folder", &directory, &error))?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| io_error("list a toolchain folder", &directory, &error))
            })
            .collect::<Result<_, _>>()?;
        children.sort();
        for child in children {
            let metadata = fs::symlink_metadata(&child)
                .map_err(|error| io_error("inspect a toolchain file", &child, &error))?;
            if metadata.is_dir() {
                queue.push(child);
            } else if metadata.is_file() {
                files.push(child);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The one platform-dependent thing in this module (rule 4): how a symlink is made, and
/// whether any of this is needed at all.
mod platform {
    use std::io;
    use std::path::Path;

    /// Case-sensitive filesystem, so the SDK's inconsistent spellings do not resolve.
    #[cfg(unix)]
    pub const NEEDS_FIXUPS: bool = true;
    /// NTFS resolves every spelling in the SDK already, and creating symlinks on Windows
    /// needs a privilege the installer explicitly does not require (user story 34).
    #[cfg(not(unix))]
    pub const NEEDS_FIXUPS: bool = false;

    #[cfg(unix)]
    pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(not(unix))]
    pub fn symlink(_target: &Path, _link: &Path) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn sdk_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Include/gl")).unwrap();
        fs::create_dir_all(root.join("Lib")).unwrap();
        fs::write(
            root.join("Include/WinDef.h"),
            "#define WINDEF\n#include <specstrings.h>\n",
        )
        .unwrap();
        fs::write(
            root.join("Include/windows.h"),
            // Mixed-case include, a backslashed one, and a directory-qualified one.
            "#include <WinDef.h>\n#include <sys\\types.h>\n#include <GL/GL.h>\n",
        )
        .unwrap();
        fs::write(root.join("Include/specstrings.h"), "/* sal */\n").unwrap();
        fs::create_dir_all(root.join("Include/Sys")).unwrap();
        fs::write(root.join("Include/Sys/Types.h"), "/* types */\n").unwrap();
        fs::write(root.join("Include/gl/gl.h"), "/* gl */\n").unwrap();
        fs::write(root.join("Lib/Kernel32.Lib"), b"lib").unwrap();
        fs::write(root.join("Lib/msvcrt.lib"), b"lib").unwrap();
        dir
    }

    fn apply_to(root: &Path) -> FixupReport {
        apply(root, &ProgressReporter::silent()).unwrap()
    }

    #[test]
    fn fixup_1_lowercases_headers_and_leaves_the_original_spelling_working() {
        let dir = sdk_tree();
        let root = dir.path();

        let report = apply_to(root);

        assert!(report.lowercased >= 1);
        // The real file is lowercase…
        assert!(
            fs::symlink_metadata(root.join("Include/windef.h"))
                .unwrap()
                .is_file()
        );
        // …and the SDK's own spelling still opens it.
        assert_eq!(
            fs::read_to_string(root.join("Include/WinDef.h")).unwrap(),
            fs::read_to_string(root.join("Include/windef.h")).unwrap()
        );
    }

    #[test]
    fn fixup_2_links_include_names_that_differ_only_in_case() {
        let dir = sdk_tree();
        let root = dir.path();

        apply_to(root);

        // `windows.h` asks for `GL/GL.h`; the tree ships `gl/gl.h`.
        assert!(
            root.join("Include/GL/GL.h").exists(),
            "tree was {:#?}",
            walk_all(root)
        );
        assert_eq!(
            fs::read_to_string(root.join("Include/GL/GL.h")).unwrap(),
            "/* gl */\n"
        );
    }

    #[test]
    fn fixup_3_makes_both_spellings_of_include_and_lib_resolve() {
        let dir = sdk_tree();
        let root = dir.path();

        let report = apply_to(root);

        assert_eq!(report.directory_links, 2);
        assert!(root.join("include").is_dir());
        assert!(root.join("lib").is_dir());
        assert!(root.join("Include").is_dir());
        assert!(root.join("Lib").is_dir());
    }

    #[test]
    fn fixup_4_rewrites_backslashes_inside_include_directives_only() {
        let dir = sdk_tree();
        let root = dir.path();
        fs::write(
            root.join("Include/tricky.h"),
            "#include <a\\b.h>\n#define MACRO(x) do { \\\n  x; \\\n} while (0)\n",
        )
        .unwrap();

        apply_to(root);

        let text = fs::read_to_string(root.join("Include/tricky.h")).unwrap();
        assert!(text.contains("#include <a/b.h>"));
        // The line-continuation backslashes are untouched.
        assert!(text.contains("do { \\\n"));
    }

    #[test]
    fn fixup_5_adds_case_spellings_for_every_import_library() {
        let dir = sdk_tree();
        let root = dir.path();

        let report = apply_to(root);

        assert!(report.lib_case_links > 0);
        for spelling in [
            "kernel32.lib",
            "KERNEL32.LIB",
            "Kernel32.Lib",
            "msvcrt.lib",
            "MSVCRT.LIB",
        ] {
            assert!(
                root.join("Lib").join(spelling).exists(),
                "Lib/{spelling} should resolve"
            );
        }
    }

    #[test]
    fn fixup_6_stubs_the_wdk_only_headers() {
        let dir = sdk_tree();
        let root = dir.path();

        apply_to(root);

        for name in ["DriverSpecs.h", "SpecStrings.h", "driverspecs.h"] {
            assert!(
                root.join("Include").join(name).exists(),
                "Include/{name} should exist"
            );
        }
        // An existing header is never replaced by a stub.
        assert_eq!(
            fs::read_to_string(root.join("Include/specstrings.h")).unwrap(),
            "/* sal */\n"
        );
    }

    /// Rule 8: the same input twice produces the same tree, and the second run changes
    /// nothing. A fix-up pass that is not idempotent would turn every retried bootstrap into
    /// a slightly different toolchain.
    #[test]
    fn applying_the_fixups_twice_changes_nothing_the_second_time() {
        let dir = sdk_tree();
        let root = dir.path();

        apply_to(root);
        let after_first = walk_all(root);
        let second = apply_to(root);
        let after_second = walk_all(root);

        assert_eq!(after_first, after_second);
        assert_eq!(second, FixupReport::default());
    }

    /// The span has to be the *name*, not merely a range that happens to rewrite correctly:
    /// fix-up 2 reads the substring out and looks it up on disk, so an off-by-one there
    /// silently stops linking anything.
    #[test]
    fn the_include_span_is_exactly_the_name_between_the_delimiters() {
        for line in [
            "#include <GL/GL.h>",
            "#include \"GL/GL.h\"",
            "  #  include <GL/GL.h>\n",
            "#include\t<GL/GL.h>   // trailing",
        ] {
            let Some((start, end)) = include_target_span(line) else {
                panic!("{line:?} is an include directive");
            };
            assert_eq!(&line[start..end], "GL/GL.h", "in {line:?}");
        }
        assert_eq!(include_target_span("int x = a < b;"), None);
    }

    #[test]
    fn include_directives_are_recognised_only_where_they_are_real() {
        assert_eq!(
            rewrite_include_lines("#include <sys\\types.h>\n"),
            "#include <sys/types.h>\n"
        );
        assert_eq!(
            rewrite_include_lines("  #  include \"a\\b\\c.h\"\n"),
            "  #  include \"a/b/c.h\"\n"
        );
        // Not an include directive: left exactly alone.
        assert_eq!(
            rewrite_include_lines("// see <sys\\types.h>\n"),
            "// see <sys\\types.h>\n"
        );
        assert_eq!(
            rewrite_include_lines("#include_next <a\\b.h>\n"),
            "#include_next <a\\b.h>\n"
        );
        assert_eq!(rewrite_include_lines("#define X \\\n"), "#define X \\\n");
    }

    #[test]
    fn lib_spellings_are_a_fixed_sorted_set() {
        assert_eq!(
            lib_spellings("Kernel32"),
            vec![
                "KERNEL32.LIB",
                "KERNEL32.Lib",
                "KERNEL32.lib",
                "Kernel32.LIB",
                "Kernel32.Lib",
                "Kernel32.lib",
                "kernel32.LIB",
                "kernel32.Lib",
                "kernel32.lib",
            ]
        );
        assert_eq!(strip_lib_extension("Kernel32.Lib"), Some("Kernel32"));
        assert_eq!(strip_lib_extension("windows.h"), None);
    }

    /// A sorted snapshot of the tree: every path, and for symlinks what they point at.
    fn walk_all(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut queue = vec![root.to_path_buf()];
        while let Some(directory) = queue.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                if metadata.is_symlink() {
                    out.push(format!(
                        "{relative} -> {}",
                        fs::read_link(&path).unwrap().display()
                    ));
                } else if metadata.is_dir() {
                    out.push(format!("{relative}/"));
                    queue.push(path);
                } else {
                    out.push(format!("{relative} ({} bytes)", metadata.len()));
                }
            }
        }
        out.sort();
        out
    }
}
