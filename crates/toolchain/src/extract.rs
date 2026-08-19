//! Disc image → MSI → CAB → files on disk, all in process.
//!
//! The order is forced by the formats: the image holds MSIs and CABs side by side, the MSI
//! says which CAB-internal name becomes which path, and the CAB holds the bytes. Only the
//! four members `docs/pinned-artifacts.md` §1 lists are touched; the other gigabyte and a
//! half of the image is never read.
//!
//! "Disc image" rather than "ISO9660" deliberately: the pinned artifact turns out to be UDF.
//! See [`crate::disc`].

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};

use crate::cabinet::{Cabinet, Wanted};
use crate::error::{ToolchainError, io_error, missing_member};
use crate::members::StagedMembers;
use crate::msi_layout::{self, PlannedFile};
use crate::pinned::IsoMember;

/// What one run of the extractor produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractionCounts {
    pub files_written: usize,
    pub bytes_written: u64,
}

/// Extract the pinned members into `sdk_root`.
///
/// Each member is a file of its own by the time it gets here - downloaded, or taken out of a
/// disc image - so the cabinets are read where they lie. Nothing is copied into scratch space
/// first, which for the two large cabinets is 73 MB of writing that no longer happens.
pub fn extract_members(
    source: &StagedMembers,
    members: &[IsoMember],
    sdk_root: &Path,
    progress: &ProgressReporter,
) -> Result<ExtractionCounts, ToolchainError> {
    fs::create_dir_all(sdk_root)
        .map_err(|error| io_error("create the toolchain folder", sdk_root, &error))?;

    check_members_present(source, members)?;

    let mut counts = ExtractionCounts::default();
    for member in members {
        progress.report(Stage::Build, format!("Unpacking the {}.", member.label));
        let member_counts = extract_member(source, member, sdk_root, progress)?;
        counts.files_written += member_counts.files_written;
        counts.bytes_written += member_counts.bytes_written;
    }
    Ok(counts)
}

/// Fail before writing anything if what arrived is not what the extraction contract
/// describes.
///
/// Worth the extra pass: the alternative is discovering the fourth member is missing after
/// half a gigabyte of headers has already been written. The log gets a listing of where the
/// member should have been, which is the one thing that makes a report about a wrong image
/// actionable.
fn check_members_present(
    source: &StagedMembers,
    members: &[IsoMember],
) -> Result<(), ToolchainError> {
    for member in members {
        for file in member.files() {
            if source.contains(file.path) {
                continue;
            }
            let described = source.describe_near(file.path);
            return Err(missing_member(file.path, "the downloaded SDK packages").context(described));
        }
    }
    Ok(())
}

fn extract_member(
    source: &StagedMembers,
    member: &IsoMember,
    sdk_root: &Path,
    progress: &ProgressReporter,
) -> Result<ExtractionCounts, ToolchainError> {
    let msi_bytes = source
        .read(member.msi.path)
        .map_err(|error| error.context(format!("member {}", member.msi.path)))?;
    let layout = msi_layout::read(Cursor::new(msi_bytes), member.label)?;

    // Group by cabinet so each one is pulled out of the image and opened exactly once.
    let mut by_cabinet: BTreeMap<String, Vec<&PlannedFile>> = BTreeMap::new();
    for file in &layout.files {
        by_cabinet
            .entry(file.cabinet.clone())
            .or_default()
            .push(file);
    }

    let mut counts = ExtractionCounts::default();
    for (cabinet_name, files) in by_cabinet {
        if cabinet_name.is_empty() {
            return Err(ToolchainError::new(
                "The Windows SDK download is not the file the installer expected.",
                format!(
                    "{} routes {} files to no cabinet at all",
                    member.msi.path,
                    files.len()
                ),
            ));
        }
        let member_path = locate_cabinet(member, &cabinet_name)?;
        let cabinet_counts = extract_from_cabinet(
            &source.file_for(&member_path),
            &files,
            sdk_root,
            member,
            progress,
        )?;
        counts.files_written += cabinet_counts.files_written;
        counts.bytes_written += cabinet_counts.bytes_written;
    }
    Ok(counts)
}

/// Match a cabinet name from the MSI's `Media` table to a path in the pinned member list.
///
/// This is where "the download is not what was pinned" gets caught: the MSI naming a cabinet
/// that `docs/pinned-artifacts.md` does not list means the image is not the one described.
fn locate_cabinet(member: &IsoMember, cabinet_name: &str) -> Result<String, ToolchainError> {
    member
        .cabs
        .iter()
        .find(|cab| {
            cab.path
                .rsplit('/')
                .next()
                .is_some_and(|base| base.eq_ignore_ascii_case(cabinet_name))
        })
        .map(|cab| cab.path.to_string())
        .ok_or_else(|| {
            missing_member(
                cabinet_name,
                &format!("the pinned member list for {}", member.msi.path),
            )
        })
}

fn extract_from_cabinet(
    staged: &Path,
    files: &[&PlannedFile],
    sdk_root: &Path,
    member: &IsoMember,
    progress: &ProgressReporter,
) -> Result<ExtractionCounts, ToolchainError> {
    let mut cabinet = Cabinet::open(staged)?;

    // Every destination directory first, then one extraction pass. The cabinet is read
    // folder by folder in a single sweep, so the whole member's worth of files is asked for
    // at once rather than one at a time (see `cabinet`).
    let mut wanted = Vec::with_capacity(files.len());
    for file in files {
        let destination = safe_destination(sdk_root, &file.relative_path)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("create a toolchain folder", parent, &error))?;
        }
        wanted.push(Wanted {
            name: &file.cab_name,
            destination,
        });
    }

    progress.report(
        Stage::Build,
        format!("Unpacking the {} - {} files.", member.label, wanted.len()),
    );
    let extracted = cabinet.extract(&wanted)?;
    Ok(ExtractionCounts {
        files_written: extracted.files,
        bytes_written: extracted.bytes,
    })
}

/// Turn an MSI-supplied relative path into an absolute one that is definitely inside
/// `sdk_root`.
///
/// The MSI comes off a download, so it is input, not truth. Every path the installer writes
/// to is derived from a root it owns; `..` in a directory table must not be able to walk out
/// of the Toolchain Cache.
fn safe_destination(sdk_root: &Path, relative: &str) -> Result<PathBuf, ToolchainError> {
    let mut destination = sdk_root.to_path_buf();
    for segment in relative
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != ".")
    {
        let candidate = Path::new(segment);
        let mut components = candidate.components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => destination.push(name),
            _ => {
                return Err(ToolchainError::new(
                    "The Windows SDK download is not the file the installer expected.",
                    format!("refusing to extract to {relative}: unsafe path segment {segment}"),
                ));
            }
        }
    }
    if destination == sdk_root {
        return Err(ToolchainError::new(
            "The Windows SDK download is not the file the installer expected.",
            format!("refusing to extract to {relative}: empty destination"),
        ));
    }
    Ok(destination)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::disc;
    use crate::pinned::ISO_MEMBERS;
    use std::fs::File;
    use std::io::{BufWriter, Read as _};

    /// Measure the pinned member table against a real image: where each member's bytes lie,
    /// how many there are, and what they hash to.
    ///
    /// This is the tool behind the per-member checksums in `docs/pinned-artifacts.md` §1. The
    /// installer fetches those members out of the middle of the image rather than the whole
    /// 1.45 GiB of it, which is only sound if each member is one run of bytes at a known
    /// offset - so this prints the extents too, and says plainly when a member is in more
    /// than one piece.
    ///
    /// Run it against an image whose SHA-256 you have already checked against the pin: these
    /// numbers become the pin, so they can only be as trustworthy as the file they came from.
    ///
    /// ```bash
    /// CIV5VP_SDK_ISO=/path/to/GRMSDK_EN_DVD.iso \
    ///   cargo test --release -p civ5vp-toolchain --lib -- --ignored --nocapture describe_the_pinned_members
    /// ```
    #[test]
    #[ignore = "needs a real Windows SDK disc image in CIV5VP_SDK_ISO"]
    fn describe_the_pinned_members() {
        use sha2::{Digest, Sha256};

        /// A sink that hashes what is written through it, so a 71 MB cabinet never needs to
        /// exist twice.
        struct Hashing(Sha256, u64);
        impl std::io::Write for Hashing {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.0.update(buffer);
                self.1 += buffer.len() as u64;
                Ok(buffer.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let Some(path) = std::env::var_os("CIV5VP_SDK_ISO") else {
            panic!("set CIV5VP_SDK_ISO to a Windows SDK 7.0 disc image");
        };
        let mut image = disc::open(&PathBuf::from(path)).unwrap();
        println!("image reads as {}", image.format());

        let mut total = 0u64;
        let mut fragmented = Vec::new();
        for member in ISO_MEMBERS {
            println!("\n=== {} ===", member.label);
            for file in member.files() {
                let name = file.path;
                let extents = image.extents(name).unwrap();
                let mut sink = Hashing(Sha256::new(), 0);
                image.copy_file_to(name, &mut sink).unwrap();
                let Hashing(hasher, bytes) = sink;
                let digest = hasher.finalize();
                let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
                total += bytes;
                let shape = match extents.as_slice() {
                    [(offset, length)] => format!("offset {offset}, {length} bytes"),
                    [] => "stored inside its own entry".to_owned(),
                    many => {
                        fragmented.push(name);
                        format!("{} pieces: {many:?}", many.len())
                    }
                };
                println!("  {name}");
                println!("    {shape}");
                println!("    {bytes} bytes, sha256 {hex}");
            }
        }
        println!("\nevery pinned member together: {total} bytes");
        assert!(
            fragmented.is_empty(),
            "these members are not one run of bytes, so one ranged request will not fetch \
             them: {fragmented:?}"
        );
    }

    /// Describe a real disc image without extracting it: which pinned members are present,
    /// what each MSI's layout says, and how each cabinet is structured.
    ///
    /// `#[ignore]`d and driven by an environment variable, because it needs an image nobody
    /// has by default. It exists for the one question that is otherwise unanswerable from a
    /// bug report - "what is actually *in* the file the user downloaded?" - and because
    /// cabinet folder sizes decide whether extraction is fast or quadratic.
    ///
    /// ```bash
    /// CIV5VP_SDK_ISO=/path/to/GRMSDK_EN_DVD.iso \
    ///   cargo test -p civ5vp-toolchain --lib -- --ignored --nocapture inspect_a_real_disc_image
    /// ```
    #[test]
    #[ignore = "needs a real Windows SDK disc image in CIV5VP_SDK_ISO"]
    fn inspect_a_real_disc_image() {
        let Some(path) = std::env::var_os("CIV5VP_SDK_ISO") else {
            panic!("set CIV5VP_SDK_ISO to a Windows SDK 7.0 disc image");
        };
        let path = PathBuf::from(path);
        let mut iso = disc::open(&path).unwrap();
        println!("image reads as {}", iso.format());

        // The tree first: if a member is missing, the next question is always "missing, or
        // somewhere else?".
        for directory in ["", "Setup"] {
            match iso.read_dir(directory) {
                Ok(entries) => {
                    println!("=== /{directory} ===");
                    for entry in entries {
                        println!(
                            "  {}{}  {} bytes",
                            entry.name,
                            if entry.is_directory { "/" } else { "" },
                            entry.size
                        );
                    }
                }
                Err(error) => println!("=== /{directory} === unreadable: {}", error.detail()),
            }
        }

        for member in ISO_MEMBERS {
            println!("\n=== {} ({}) ===", member.msi.path, member.label);
            for cab in member.cabs {
                println!(
                    "  cabinet listed: {} present={}",
                    cab.path,
                    iso.contains(cab.path)
                );
            }
            if !iso.contains(member.msi.path) {
                println!("  MSI ABSENT");
                continue;
            }
            // Report and carry on rather than stopping: an image that is truncated or
            // damaged usually is so in one place, and "here is everything, and here is the
            // one thing that failed" is the whole point of this test.
            let bytes = match iso.read_file(member.msi.path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    println!("  MSI UNREADABLE: {}", error.detail());
                    continue;
                }
            };
            println!("  MSI is {} bytes", bytes.len());
            let layout = match msi_layout::read(Cursor::new(bytes), member.label) {
                Ok(layout) => layout,
                Err(error) => {
                    println!("  MSI UNPARSEABLE: {}", error.detail());
                    continue;
                }
            };
            println!("  cabinets named by Media: {:?}", layout.cabinets);
            println!("  files: {}", layout.files.len());
            for file in layout.files.iter().take(4) {
                println!(
                    "    {} -> {} (cab {})",
                    file.cab_name, file.relative_path, file.cabinet
                );
            }

            // Folder sizes say whether extraction is one sweep or a quadratic one, and the
            // cross-check is what makes our sequential reader trustworthy on LZX: the fast
            // suite can only build MSZIP fixtures, so the real cabinets are where `cab`'s
            // reader gets to be the oracle for the compression these actually use.
            let staging = std::env::temp_dir().join("civ5vp-inspect");
            let _ = fs::create_dir_all(&staging);
            for cab_path in member.cabs.iter().map(|cab| cab.path) {
                if !iso.contains(cab_path) {
                    continue;
                }
                let staged = staging.join("inspect.cab");
                let mut out = BufWriter::new(File::create(&staged).unwrap());
                let copied = iso.copy_file_to(cab_path, &mut out);
                drop(out);
                if let Err(error) = copied {
                    println!("  {cab_path}: UNREADABLE: {}", error.detail());
                    continue;
                }

                let mut reference =
                    cab::Cabinet::new(std::io::BufReader::new(File::open(&staged).unwrap()))
                        .unwrap();
                let folders: Vec<(usize, u64, String)> = reference
                    .folder_entries()
                    .map(|folder| {
                        (
                            folder.file_entries().count(),
                            folder
                                .file_entries()
                                .map(|file| u64::from(file.uncompressed_size()))
                                .sum(),
                            format!("{:?}", folder.compression_type()),
                        )
                    })
                    .collect();
                println!("  {cab_path}: {} folders", folders.len());
                for folder in &folders {
                    println!("    {} files, {} bytes, {}", folder.0, folder.1, folder.2);
                }

                // A handful spread through the folder - including the last, which is the one
                // a block-boundary mistake shifts.
                let names: Vec<String> = reference
                    .folder_entries()
                    .flat_map(|folder| folder.file_entries())
                    .map(|file| file.name().to_string())
                    .collect();
                let sampled: Vec<&String> = names
                    .iter()
                    .step_by((names.len() / 5).max(1))
                    .chain(names.last())
                    .collect();

                let mut mine = Cabinet::open(&staged).unwrap();
                for name in &sampled {
                    let destination = staging.join("member.out");
                    mine.extract(&[Wanted {
                        name,
                        destination: destination.clone(),
                    }])
                    .unwrap();
                    let mut expected = Vec::new();
                    reference
                        .read_file(name)
                        .unwrap()
                        .read_to_end(&mut expected)
                        .unwrap();
                    assert_eq!(
                        fs::read(&destination).unwrap(),
                        expected,
                        "{name} in {cab_path}"
                    );
                }
                println!("    {} members agree with the cab crate", sampled.len());
                let _ = fs::remove_dir_all(&staging);
                let _ = fs::create_dir_all(&staging);
            }
            let _ = fs::remove_dir_all(&staging);
        }
    }

    #[test]
    fn a_relative_path_lands_under_the_toolchain_root() {
        let root = Path::new("/cache/winsdk");
        assert_eq!(
            safe_destination(root, "Include/windows.h").unwrap(),
            Path::new("/cache/winsdk/Include/windows.h")
        );
        assert_eq!(
            safe_destination(root, "Include\\gl\\gl.h").unwrap(),
            Path::new("/cache/winsdk/Include/gl/gl.h")
        );
    }

    #[test]
    fn a_path_that_tries_to_escape_the_toolchain_root_is_refused() {
        let root = Path::new("/cache/winsdk");
        for hostile in ["../outside.h", "Include/../../outside.h", "..", ""] {
            let Err(error) = safe_destination(root, hostile) else {
                panic!("{hostile} should be refused");
            };
            assert!(error.detail().contains("refusing to extract"));
        }
    }

    /// An absolute-looking path is not refused but is still contained: the leading separator
    /// is just an empty segment, so it lands under the toolchain root like any other name.
    #[test]
    fn an_absolute_looking_path_still_lands_inside_the_toolchain_root() {
        let root = Path::new("/cache/winsdk");
        assert_eq!(
            safe_destination(root, "/etc/passwd").unwrap(),
            Path::new("/cache/winsdk/etc/passwd")
        );
    }

    #[test]
    fn a_cabinet_the_pinned_list_does_not_mention_is_refused() {
        let member = &ISO_MEMBERS[0];
        assert_eq!(
            locate_cabinet(member, "cab1.cab").unwrap(),
            "Setup/WinSDK/cab1.cab"
        );
        // The MSI's spelling may differ in case from the image's.
        assert_eq!(
            locate_cabinet(member, "CAB1.CAB").unwrap(),
            "Setup/WinSDK/cab1.cab"
        );
        let error = locate_cabinet(member, "cab9.cab").unwrap_err();
        assert!(error.detail().contains("cab9.cab"));
    }
}
