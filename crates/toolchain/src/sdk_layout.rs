//! Finding `Include/` and `Lib/` in the extracted tree.
//!
//! They are not where a first reading of `docs/pinned-artifacts.md` suggests. The MSIs place
//! everything under the path Windows would install to, so the real tree is
//!
//! ```text
//! <toolchain>/Program Files/Microsoft SDKs/Windows/v7.0/Include/windows.h
//! <toolchain>/Program Files/Microsoft SDKs/Windows/v7.0/Lib/Kernel32.Lib
//! <toolchain>/Program Files/Microsoft SDKs/Windows/v7.0/Lib/x64/Kernel32.Lib
//! ```
//!
//! and the VC9 CRT lands somewhere else again. Honouring the MSI's mapping means not
//! flattening that (`docs/pinned-artifacts.md` §1), so everything downstream — the fix-ups,
//! the verification, and eventually the compiler's include path — asks *where* rather than
//! assuming. One walk answers it.

use std::path::{Path, PathBuf};

use crate::error::{ToolchainError, io_error};

/// How deep to look. The real layout puts `Include` five levels down; a search that goes much
/// further is walking the headers themselves.
const MAX_DEPTH: usize = 8;

/// The `Include` and `Lib` directories of an extracted toolchain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SdkRoots {
    /// Every directory called `include`, in sorted order, outermost only.
    pub include: Vec<PathBuf>,
    /// Every directory called `lib`, in sorted order, outermost only.
    pub lib: Vec<PathBuf>,
}

impl SdkRoots {
    /// Whether the extraction produced anything to work with at all.
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.lib.is_empty()
    }
}

/// Search `root` for directories named `include` or `lib`, ignoring case.
///
/// Outermost only: `Lib/x64` is inside `Lib` and is not a second root, it is part of the
/// first one. That matters for fix-up 5, which would otherwise add case symlinks twice.
pub fn find(root: &Path) -> Result<SdkRoots, ToolchainError> {
    let mut roots = SdkRoots::default();
    walk(root, 0, &mut roots)?;
    roots.include.sort();
    roots.lib.sort();
    Ok(roots)
}

fn walk(directory: &Path, depth: usize, roots: &mut SdkRoots) -> Result<(), ToolchainError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = std::fs::read_dir(directory)
        .map_err(|error| io_error("list the toolchain folder", directory, &error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| io_error("list the toolchain folder", directory, &error))?;
        let path = entry.path();
        // Not `is_dir`: a symlink to a directory is one of fix-up 3's own outputs, and
        // following it would find the same tree twice.
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| io_error("inspect a toolchain folder", &path, &error))?;
        if !metadata.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.eq_ignore_ascii_case("include") {
            roots.include.push(path);
        } else if name.eq_ignore_ascii_case("lib") {
            roots.lib.push(path);
        } else {
            walk(&path, depth + 1, roots)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;

    /// The layout the real MSIs actually produce.
    #[test]
    fn the_sdk_roots_are_found_however_deep_the_msi_buried_them() {
        let dir = tempfile::tempdir().unwrap();
        let sdk = dir.path().join("Program Files/Microsoft SDKs/Windows/v7.0");
        fs::create_dir_all(sdk.join("Include/gl")).unwrap();
        fs::create_dir_all(sdk.join("Lib/x64")).unwrap();
        fs::create_dir_all(
            dir.path()
                .join("Program Files/Microsoft Visual Studio 9.0/VC/include"),
        )
        .unwrap();

        let roots = find(dir.path()).unwrap();

        assert_eq!(
            roots.include,
            vec![
                sdk.join("Include"),
                dir.path()
                    .join("Program Files/Microsoft Visual Studio 9.0/VC/include"),
            ]
        );
        assert_eq!(roots.lib, vec![sdk.join("Lib")]);
    }

    /// `Lib/x64` is part of `Lib`, not a root of its own.
    #[test]
    fn nested_directories_of_the_same_name_are_not_separate_roots() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Lib/x64/lib")).unwrap();
        fs::create_dir_all(dir.path().join("Include/crt/include")).unwrap();

        let roots = find(dir.path()).unwrap();

        assert_eq!(roots.include, vec![dir.path().join("Include")]);
        assert_eq!(roots.lib, vec![dir.path().join("Lib")]);
    }

    #[test]
    fn an_extraction_with_neither_is_reported_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Bin")).unwrap();

        assert!(find(dir.path()).unwrap().is_empty());
    }
}
