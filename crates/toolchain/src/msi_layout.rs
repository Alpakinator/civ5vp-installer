//! Reading the layout an MSI describes: which name inside which CAB becomes which file on
//! disk.
//!
//! `docs/pinned-artifacts.md` §1: "Each MSI carries the mapping from CAB-internal names to
//! real file paths - that mapping must be honoured, not guessed." That mapping is four
//! tables:
//!
//! - `File` - `File` (the key, which is also the name inside the CAB), `Component_`,
//!   `FileName`, `Sequence`.
//! - `Component` - `Component`, `Directory_`.
//! - `Directory` - `Directory`, `Directory_Parent`, `DefaultDir`.
//! - `Media` - `LastSequence`, `Cabinet`: which CAB holds which range of sequences.
//!
//! Following those four is the whole of the contract. Nothing here looks at a file's
//! extension or guesses from a name.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use msi::{Package, Select};

use crate::error::ToolchainError;

/// One file, as the MSI says it should land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// The name to ask the CAB for. For a compressed package this is the `File` key.
    pub cab_name: String,
    /// Where it goes, relative to the extraction root: `Include/windows.h`.
    pub relative_path: String,
    /// Which CAB holds it, resolved through the `Media` table.
    pub cabinet: String,
    pub sequence: i32,
}

/// Everything the extractor needs out of one MSI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsiLayout {
    /// Sorted by sequence, which is the order the files sit in the cabinets.
    pub files: Vec<PlannedFile>,
    /// Cabinet names in `Media` order, as the MSI spells them.
    pub cabinets: Vec<String>,
}

/// Read the layout out of an open MSI.
pub fn read<F: Read + Seek>(reader: F, label: &str) -> Result<MsiLayout, ToolchainError> {
    let mut package = Package::open(reader).map_err(|error| unreadable(label, &error))?;

    let directories = read_directories(&mut package, label)?;
    let directory_paths = resolve_directory_paths(&directories);
    let components = read_components(&mut package, label)?;
    let media = read_media(&mut package, label)?;

    let mut files = Vec::new();
    for row in package
        .select_rows(Select::table("File"))
        .map_err(|error| unreadable(label, &error))?
    {
        let key = named_string(&row, "File");
        let component = named_string(&row, "Component_");
        let file_name = named_string(&row, "FileName");
        let sequence = named_int(&row, "Sequence");
        let (Some(key), Some(component), Some(file_name), Some(sequence)) =
            (key, component, file_name, sequence)
        else {
            continue;
        };

        // A file whose component names a directory the Directory table does not define is a
        // broken package, not something to place at a guessed location.
        let directory_key = match components.get(&component) {
            Some(directory) => directory,
            None => continue,
        };
        let prefix = directory_paths
            .get(directory_key)
            .cloned()
            .unwrap_or_default();

        let name = long_name(&file_name);
        let relative_path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let cabinet = cabinet_for(&media, sequence);

        files.push(PlannedFile {
            cab_name: key,
            relative_path,
            cabinet,
            sequence,
        });
    }

    files.sort_by(|a, b| {
        a.sequence
            .cmp(&b.sequence)
            .then(a.cab_name.cmp(&b.cab_name))
    });
    let cabinets = media
        .iter()
        .map(|entry| entry.cabinet.clone())
        .filter(|name| !name.is_empty())
        .collect();

    Ok(MsiLayout { files, cabinets })
}

/// A `Media` row: everything up to `last_sequence` that is not claimed by an earlier row
/// lives in `cabinet`.
#[derive(Debug, Clone)]
struct MediaEntry {
    last_sequence: i32,
    cabinet: String,
}

fn read_media<F: Read + Seek>(
    package: &mut Package<F>,
    label: &str,
) -> Result<Vec<MediaEntry>, ToolchainError> {
    let mut media: Vec<MediaEntry> = Vec::new();
    for row in package
        .select_rows(Select::table("Media"))
        .map_err(|error| unreadable(label, &error))?
    {
        let last_sequence = named_int(&row, "LastSequence").unwrap_or(0);
        // `Cabinet` is `#name` when the CAB is embedded as an MSI stream and a plain file
        // name when it sits beside the MSI. The SDK ISO uses the latter throughout; the `#`
        // form is stripped so an embedded package at least names its stream correctly.
        let cabinet = named_string(&row, "Cabinet").unwrap_or_default();
        media.push(MediaEntry {
            last_sequence,
            cabinet: cabinet.trim_start_matches('#').to_string(),
        });
    }
    media.sort_by_key(|entry| entry.last_sequence);
    Ok(media)
}

fn cabinet_for(media: &[MediaEntry], sequence: i32) -> String {
    media
        .iter()
        .find(|entry| sequence <= entry.last_sequence)
        .map(|entry| entry.cabinet.clone())
        .unwrap_or_default()
}

fn read_components<F: Read + Seek>(
    package: &mut Package<F>,
    label: &str,
) -> Result<BTreeMap<String, String>, ToolchainError> {
    let mut components = BTreeMap::new();
    for row in package
        .select_rows(Select::table("Component"))
        .map_err(|error| unreadable(label, &error))?
    {
        let key = named_string(&row, "Component");
        let directory = named_string(&row, "Directory_");
        if let (Some(key), Some(directory)) = (key, directory) {
            components.insert(key, directory);
        }
    }
    Ok(components)
}

/// `(key, parent key, DefaultDir)`.
fn read_directories<F: Read + Seek>(
    package: &mut Package<F>,
    label: &str,
) -> Result<Vec<(String, String, String)>, ToolchainError> {
    let mut directories = Vec::new();
    for row in package
        .select_rows(Select::table("Directory"))
        .map_err(|error| unreadable(label, &error))?
    {
        let key = named_string(&row, "Directory");
        let parent = named_string(&row, "Directory_Parent").unwrap_or_default();
        let default_dir = named_string(&row, "DefaultDir").unwrap_or_default();
        if let Some(key) = key {
            directories.push((key, parent, default_dir));
        }
    }
    Ok(directories)
}

/// Walk each directory up to its root, building a relative path.
///
/// Four MSI conventions matter here. `DefaultDir` may be `short|long` - the long name wins.
/// A `DefaultDir` of `.` means "no folder of its own", which is how installers hang several
/// keys off one physical directory. `DefaultDir` may carry a `target:source` pair; only the
/// target half describes the installed layout. And the row with no parent is the install
/// root - conventionally `TARGETDIR`, `DefaultDir` `SourceDir` - whose name is never part of
/// an installed path at all.
fn resolve_directory_paths(directories: &[(String, String, String)]) -> BTreeMap<String, String> {
    let by_key: BTreeMap<&str, (&str, &str)> = directories
        .iter()
        .map(|(key, parent, default)| (key.as_str(), (parent.as_str(), default.as_str())))
        .collect();

    let mut resolved = BTreeMap::new();
    for (key, _, _) in directories {
        let mut segments: Vec<String> = Vec::new();
        let mut current = key.as_str();
        // Bounded by the number of rows: a `Directory_Parent` cycle would otherwise hang the
        // installer, and a malformed package must not be able to do that.
        for _ in 0..=directories.len() {
            let Some((parent, default)) = by_key.get(current).copied() else {
                break;
            };
            let is_install_root = parent.is_empty() || parent == current;
            if !is_install_root {
                let name = long_name(target_half(default));
                if !name.is_empty() && name != "." {
                    segments.push(name);
                }
            }
            if is_install_root {
                break;
            }
            current = parent;
        }
        segments.reverse();
        resolved.insert(key.clone(), segments.join("/"));
    }
    resolved
}

/// `short|long` → `long`; anything else unchanged.
fn long_name(value: &str) -> String {
    match value.split_once('|') {
        Some((_short, long)) => long.to_string(),
        None => value.to_string(),
    }
}

/// `target:source` → `target`; anything else unchanged.
fn target_half(value: &str) -> &str {
    match value.split_once(':') {
        Some((target, _source)) => target,
        None => value,
    }
}

/// `msi::Row` only offers a panicking `Index`, which is unacceptable on a path a
/// downloaded file can reach. These three read cells by *name*, bounds-checked - which also
/// means a package whose columns are ordered unusually still reads correctly.
fn value(row: &msi::Row, column: &str) -> Option<msi::Value> {
    let index = row.columns().iter().position(|c| c.name() == column)?;
    (index < row.len()).then(|| row[index].clone())
}

fn named_string(row: &msi::Row, column: &str) -> Option<String> {
    match value(row, column) {
        Some(msi::Value::Str(text)) => Some(text),
        _ => None,
    }
}

fn named_int(row: &msi::Row, column: &str) -> Option<i32> {
    match value(row, column) {
        Some(msi::Value::Int(number)) => Some(number),
        _ => None,
    }
}

fn unreadable(label: &str, error: &std::io::Error) -> ToolchainError {
    ToolchainError::new(
        "The Windows SDK download could not be unpacked. Clear the installer's data folder \
         and try again.",
        format!("reading the {label} installer database failed: {error}"),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_fixtures::package::{DirectoryRow, FileRow, build};
    use std::io::Cursor;

    fn sdk_shaped_msi() -> Vec<u8> {
        build(
            &[
                DirectoryRow {
                    key: "TARGETDIR",
                    parent: "",
                    default_dir: "SourceDir",
                },
                // `.` - a key that adds no path segment of its own, which is how the real SDK
                // packages hang their roots off TARGETDIR.
                DirectoryRow {
                    key: "SDKROOT",
                    parent: "TARGETDIR",
                    default_dir: ".",
                },
                DirectoryRow {
                    key: "IncludeDir",
                    parent: "SDKROOT",
                    default_dir: "Include",
                },
                // `short|long`, the form the SDK uses for anything over 8.3.
                DirectoryRow {
                    key: "GlDir",
                    parent: "IncludeDir",
                    default_dir: "gl|OpenGL",
                },
                DirectoryRow {
                    key: "LibDir",
                    parent: "SDKROOT",
                    default_dir: "Lib",
                },
            ],
            &[
                ("IncludeComponent", "IncludeDir"),
                ("GlComponent", "GlDir"),
                ("LibComponent", "LibDir"),
            ],
            &[
                FileRow {
                    key: "windows.h",
                    file_name: "windows.h",
                    component: "IncludeComponent",
                    sequence: 1,
                },
                FileRow {
                    key: "file2.h",
                    file_name: "GLAUX~1.H|glaux.h",
                    component: "GlComponent",
                    sequence: 2,
                },
                FileRow {
                    key: "kernel32.lib",
                    file_name: "kernel32.lib",
                    component: "LibComponent",
                    sequence: 3,
                },
            ],
            &[(3, "cab1.cab")],
        )
    }

    #[test]
    fn the_directory_tree_becomes_a_relative_path_per_file() {
        let layout = read(Cursor::new(sdk_shaped_msi()), "test").unwrap();

        let paths: Vec<&str> = layout
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec![
                "Include/windows.h",
                "Include/OpenGL/glaux.h",
                "Lib/kernel32.lib"
            ]
        );
    }

    /// The point of the whole module: the name to ask the CAB for is the `File` key, which is
    /// nothing like the name on disk.
    #[test]
    fn the_cab_name_comes_from_the_file_key_not_from_the_file_name() {
        let layout = read(Cursor::new(sdk_shaped_msi()), "test").unwrap();

        let glaux = layout
            .files
            .iter()
            .find(|file| file.relative_path.ends_with("glaux.h"))
            .unwrap();
        assert_eq!(glaux.cab_name, "file2.h");
        assert_eq!(glaux.relative_path, "Include/OpenGL/glaux.h");
    }

    #[test]
    fn a_dot_directory_contributes_no_path_segment() {
        let layout = read(Cursor::new(sdk_shaped_msi()), "test").unwrap();
        assert!(
            layout
                .files
                .iter()
                .all(|file| !file.relative_path.contains("SourceDir")
                    && !file.relative_path.starts_with('/'))
        );
    }

    #[test]
    fn every_file_is_routed_to_the_cabinet_its_sequence_falls_in() {
        let layout = read(Cursor::new(sdk_shaped_msi()), "test").unwrap();

        assert_eq!(layout.cabinets, vec!["cab1.cab".to_string()]);
        assert!(layout.files.iter().all(|file| file.cabinet == "cab1.cab"));
    }

    #[test]
    fn several_media_rows_split_the_sequence_range_between_cabinets() {
        let media = vec![
            MediaEntry {
                last_sequence: 10,
                cabinet: "cab1.cab".to_string(),
            },
            MediaEntry {
                last_sequence: 25,
                cabinet: "cab2.cab".to_string(),
            },
        ];
        assert_eq!(cabinet_for(&media, 1), "cab1.cab");
        assert_eq!(cabinet_for(&media, 10), "cab1.cab");
        assert_eq!(cabinet_for(&media, 11), "cab2.cab");
        assert_eq!(cabinet_for(&media, 25), "cab2.cab");
        assert_eq!(cabinet_for(&media, 26), "");
    }

    #[test]
    fn msi_name_conventions_are_decoded() {
        assert_eq!(long_name("GLAUX~1.H|glaux.h"), "glaux.h");
        assert_eq!(long_name("windows.h"), "windows.h");
        assert_eq!(target_half("Include:src_include"), "Include");
        assert_eq!(target_half("Include"), "Include");
    }

    #[test]
    fn a_parent_cycle_terminates_instead_of_hanging() {
        let directories = vec![
            ("A".to_string(), "B".to_string(), "a".to_string()),
            ("B".to_string(), "A".to_string(), "b".to_string()),
        ];
        let resolved = resolve_directory_paths(&directories);
        assert!(resolved.contains_key("A"));
    }
}
