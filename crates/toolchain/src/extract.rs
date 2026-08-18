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
use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};

use crate::cabinet::{Cabinet, Wanted};
use crate::disc::{self, Disc};
use crate::error::{ToolchainError, io_error, missing_member};
use crate::msi_layout::{self, PlannedFile};
use crate::pinned::{IsoMember, member_cache_name};

/// Where the pinned members' bytes come from.
///
/// The seam exists because there are two honest answers, and the extraction after them is
/// identical: a whole disc image the installer has a copy of, or the eleven member files it
/// fetched out of the middle of that image without ever downloading the other 1.35 GiB.
pub trait MemberSource {
    fn contains(&mut self, path: &str) -> bool;

    fn read(&mut self, path: &str) -> Result<Vec<u8>, ToolchainError>;

    fn copy_to(&mut self, path: &str, out: &mut dyn Write) -> Result<u64, ToolchainError>;

    /// Where this member already is on disk, when it is a file in its own right. The
    /// cabinets are up to 43 MB, and copying one into scratch space that is already a file is
    /// pure waste — so the file path is offered, and only a real copy is deleted afterwards.
    fn local_path(&self, _path: &str) -> Option<PathBuf> {
        None
    }

    /// What this source is and what it holds around `path` — the log line for a missing
    /// member, where "what was actually there?" is the only question worth answering.
    fn describe_near(&mut self, path: &str) -> String;
}

impl<R: Read + Seek> MemberSource for Disc<R> {
    fn contains(&mut self, path: &str) -> bool {
        Disc::contains(self, path)
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, ToolchainError> {
        Disc::read_file(self, path)
    }

    fn copy_to(&mut self, path: &str, out: &mut dyn Write) -> Result<u64, ToolchainError> {
        // `&mut out` rather than `out`: the disc readers are generic over a *sized* writer,
        // and a `&mut` to an unsized one is exactly that.
        let mut out = out;
        Disc::copy_file_to(self, path, &mut out)
    }

    fn describe_near(&mut self, path: &str) -> String {
        let (directory, _) = path.rsplit_once('/').unwrap_or(("", path));
        let siblings = self
            .read_dir(directory)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| entry.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|_| "<no such folder>".to_string());
        format!(
            "the image reads as {}; {directory} contains: {siblings}",
            self.format()
        )
    }
}

/// The members as files, each already checked against its own SHA-256 on the way in.
///
/// Named by [`PinnedMember::cache_name`], which flattens the path inside the image — the two
/// different `cab1.cab`s must not land on top of each other.
pub struct StagedMembers {
    directory: PathBuf,
}

impl StagedMembers {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn file_for(&self, path: &str) -> PathBuf {
        self.directory.join(member_cache_name(path))
    }
}

impl MemberSource for StagedMembers {
    fn contains(&mut self, path: &str) -> bool {
        self.file_for(path).is_file()
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, ToolchainError> {
        let file = self.file_for(path);
        fs::read(&file).map_err(|error| io_error("read a downloaded SDK package", &file, &error))
    }

    fn copy_to(&mut self, path: &str, out: &mut dyn Write) -> Result<u64, ToolchainError> {
        let file = self.file_for(path);
        let mut handle = File::open(&file)
            .map_err(|error| io_error("read a downloaded SDK package", &file, &error))?;
        std::io::copy(&mut handle, out)
            .map_err(|error| io_error("read a downloaded SDK package", &file, &error))
    }

    fn local_path(&self, path: &str) -> Option<PathBuf> {
        let file = self.file_for(path);
        file.is_file().then_some(file)
    }

    fn describe_near(&mut self, path: &str) -> String {
        let present: Vec<String> = fs::read_dir(&self.directory)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "{path} was expected at {}; that folder holds: {}",
            self.file_for(path).display(),
            present.join(", ")
        )
    }
}

/// What one run of the extractor produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractionCounts {
    pub files_written: usize,
    pub bytes_written: u64,
}

/// Extract the pinned members into `sdk_root`, wherever their bytes are coming from.
///
/// `staging` is scratch space for the CABs on their way out of a disc image — they are up to
/// a couple of hundred megabytes each, which is more than belongs in memory. It is emptied of
/// each CAB as soon as that CAB is done with; a member that is already a file of its own is
/// read where it lies and never copied.
pub fn extract_members(
    source: &mut dyn MemberSource,
    members: &[IsoMember],
    sdk_root: &Path,
    staging: &Path,
    progress: &ProgressReporter,
) -> Result<ExtractionCounts, ToolchainError> {
    fs::create_dir_all(sdk_root)
        .map_err(|error| io_error("create the toolchain folder", sdk_root, &error))?;
    fs::create_dir_all(staging)
        .map_err(|error| io_error("create a temporary folder", staging, &error))?;

    check_members_present(source, members)?;

    let mut counts = ExtractionCounts::default();
    for member in members {
        progress.report(Stage::Build, format!("Unpacking the {}.", member.label));
        let member_counts = extract_member(source, member, sdk_root, staging, progress)?;
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
    source: &mut dyn MemberSource,
    members: &[IsoMember],
) -> Result<(), ToolchainError> {
    for member in members {
        for file in member.files() {
            if source.contains(file.path) {
                continue;
            }
            let described = source.describe_near(file.path);
            return Err(missing_member(file.path, "the disc image").context(described));
        }
    }
    Ok(())
}

fn extract_member(
    source: &mut dyn MemberSource,
    member: &IsoMember,
    sdk_root: &Path,
    staging: &Path,
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
        let staged = stage_cabinet(source, &member_path, staging)?;
        let result = extract_from_cabinet(staged.path(), &files, sdk_root, member, progress);
        // A copy is scratch: remove it whether or not the extraction worked, so a failed
        // bootstrap does not leave hundreds of megabytes behind. A member that was already a
        // file of its own is not ours to delete — it is what a retry resumes from.
        staged.discard();
        let cabinet_counts = result?;
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

/// A cabinet ready for the `cab` crate to seek in, and whether cleaning up means deleting it.
enum StagedCabinet {
    /// Copied out of a disc image into scratch space.
    Copy(PathBuf),
    /// Already a file in its own right — the member the installer downloaded.
    Existing(PathBuf),
}

impl StagedCabinet {
    fn path(&self) -> &Path {
        match self {
            Self::Copy(path) | Self::Existing(path) => path,
        }
    }

    fn discard(self) {
        if let Self::Copy(path) = self {
            let _ = fs::remove_file(path);
        }
    }
}

/// Put one CAB where the `cab` crate can seek in it — which for a downloaded member is
/// exactly where it already is.
fn stage_cabinet(
    source: &mut dyn MemberSource,
    member_path: &str,
    staging: &Path,
) -> Result<StagedCabinet, ToolchainError> {
    if let Some(existing) = source.local_path(member_path) {
        return Ok(StagedCabinet::Existing(existing));
    }
    let staged = disc::staged_path(staging, member_path);
    let file = File::create(&staged)
        .map_err(|error| io_error("write a temporary cabinet", &staged, &error))?;
    let mut writer = BufWriter::new(file);
    source
        .copy_to(member_path, &mut writer)
        .map_err(|error| error.context(format!("member {member_path}")))?;
    drop(writer);
    Ok(StagedCabinet::Copy(staged))
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
        format!(
            "Unpacking the {} — {} files from {}.",
            member.label,
            wanted.len(),
            staged.file_name().unwrap_or_default().display()
        ),
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
    use crate::pinned::ISO_MEMBERS;

    /// Measure the pinned member table against a real image: where each member's bytes lie,
    /// how many there are, and what they hash to.
    ///
    /// This is the tool behind the per-member checksums in `docs/pinned-artifacts.md` §1. The
    /// installer fetches those members out of the middle of the image rather than the whole
    /// 1.45 GiB of it, which is only sound if each member is one run of bytes at a known
    /// offset — so this prints the extents too, and says plainly when a member is in more
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
    /// bug report — "what is actually *in* the file the user downloaded?" — and because
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

                // A handful spread through the folder — including the last, which is the one
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
