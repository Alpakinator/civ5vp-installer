//! The pinned members of the Windows SDK image, as files in the Toolchain Cache.
//!
//! The build reads eleven files out of a 1.45 GiB disc image (`docs/pinned-artifacts.md` §1).
//! Keeping those eleven — about 102 MiB — is enough to unpack the SDK again at any time, so
//! they are what the cache holds and the image is not.
//!
//! There are two ways to come by them, and this module owns both:
//!
//! - **Fetch** each one as a windowed download, which is what a first bootstrap does.
//! - **Harvest** them out of a whole image an earlier version of the installer downloaded.
//!   That version kept the image because it extracted straight from it; this one does not need
//!   it, and 1.35 GiB of a player's disk is not ours to sit on. Harvesting reads the members
//!   out locally — no network at all — and only then is the image removed.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};
use sha2::{Digest, Sha256};

use crate::disc::{self, Disc};
use crate::download::{ByteSource, fetch_member, hash_file};
use crate::error::{ToolchainError, io_error, missing_member};
use crate::pinned::{IsoMember, PinnedDownload, PinnedMember, member_cache_name};

/// The members as files in one directory, each already checked against its own SHA-256 on the
/// way in.
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

    /// Where a member lives, whether or not it is there yet.
    pub fn file_for(&self, path: &str) -> PathBuf {
        self.directory.join(member_cache_name(path))
    }

    pub fn contains(&self, path: &str) -> bool {
        self.file_for(path).is_file()
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, ToolchainError> {
        let file = self.file_for(path);
        fs::read(&file).map_err(|error| io_error("read a downloaded SDK package", &file, &error))
    }

    /// What this folder holds around `path` — the log line for a missing member, where "what
    /// was actually there?" is the only question worth answering.
    pub fn describe_near(&self, path: &str) -> String {
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

    /// Whether every one of `members` is present and hashes to what it should.
    ///
    /// The hash and not just the name, because this is what an image is deleted on the
    /// strength of.
    fn all_verified(&self, members: &[IsoMember]) -> bool {
        members.iter().flat_map(IsoMember::files).all(|file| {
            let path = self.file_for(file.path);
            path.is_file() && hash_file(&path).is_ok_and(|digest| digest == file.sha256)
        })
    }
}

/// Fetch whatever is missing, one windowed download at a time.
///
/// Idempotent: a member already present and verified costs nothing, so an interrupted
/// bootstrap picks up where it stopped.
pub fn fetch_missing(
    source: &dyn ByteSource,
    image: &PinnedDownload,
    members: &[IsoMember],
    into: &Path,
    progress: &ProgressReporter,
) -> Result<StagedMembers, ToolchainError> {
    let total: u64 = members
        .iter()
        .flat_map(IsoMember::files)
        .map(|file| file.bytes)
        .sum();
    progress.report(
        Stage::Build,
        format!(
            "Getting the {} packages the Windows SDK build needs — {} MB, rather than the \
             1.4 GB image they sit inside.",
            members.iter().flat_map(IsoMember::files).count(),
            total / (1024 * 1024)
        ),
    );

    for member in members {
        let pieces = member.files().count();
        for (index, file) in member.files().enumerate() {
            // Named for what it is rather than for what it is called on disk: the log a
            // player reads should say "the VC9 CRT", not `vc_stdx86-vc_stdx86.cab`.
            let label = if pieces == 1 {
                format!("the {}", member.label)
            } else {
                format!("the {} ({} of {pieces})", member.label, index + 1)
            };
            fetch_member(source, image, file, &label, into, progress)?;
        }
    }
    Ok(StagedMembers::new(into.to_path_buf()))
}

/// Read the members out of a whole disc image on disk, and then remove the image.
///
/// Returns how many bytes the cache got back. Nothing is deleted until every member is on
/// disk and hashes correctly: the image is the only copy of those bytes the machine has, and
/// swapping it for an incomplete set would turn a full cache into a download.
pub fn harvest_from_image(
    image_path: &Path,
    members: &[IsoMember],
    into: &Path,
    progress: &ProgressReporter,
) -> Result<u64, ToolchainError> {
    let staged = StagedMembers::new(into.to_path_buf());
    if !staged.all_verified(members) {
        fs::create_dir_all(into)
            .map_err(|error| io_error("create the downloads folder", into, &error))?;
        progress.report(
            Stage::Build,
            "Taking the packages the build needs out of the Windows SDK image already here — \
             nothing is downloaded.",
        );
        let mut image = disc::open(image_path)?;
        for member in members {
            for file in member.files() {
                if staged.contains(file.path)
                    && hash_file(&staged.file_for(file.path))? == file.sha256
                {
                    continue;
                }
                if !image.contains(file.path) {
                    return Err(missing_member(file.path, "the disc image already here")
                        .context(describe_near(&mut image, file.path)));
                }
                copy_member(&mut image, file, &staged.file_for(file.path))?;
            }
        }
    }

    // Everything the image was kept for is now here, proven. The image itself is 1.35 GiB of
    // bytes nothing will read again.
    if !staged.all_verified(members) {
        return Err(ToolchainError::new(
            "The Windows SDK image on this computer is not the one the installer expects.",
            format!("{} did not yield every pinned member", image_path.display()),
        ));
    }
    let freed = fs::metadata(image_path).map(|data| data.len()).unwrap_or(0);
    fs::remove_file(image_path)
        .map_err(|error| io_error("remove a disc image no longer needed", image_path, &error))?;
    Ok(freed)
}

/// What the image holds around a member that is not where it should be.
///
/// An old image that cannot produce a member is the one case where "what is actually in this
/// file?" decides whether the answer is "a different SDK" or "a truncated download", and
/// neither is guessable from the failure alone.
fn describe_near<R: Read + Seek>(image: &mut Disc<R>, path: &str) -> String {
    let (directory, _) = path.rsplit_once('/').unwrap_or(("", path));
    let siblings = image
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
        image.format()
    )
}

/// Copy one member out of the image, checking it as it goes and only then giving it its name.
fn copy_member<R: Read + Seek>(
    image: &mut Disc<R>,
    member: &PinnedMember,
    destination: &Path,
) -> Result<(), ToolchainError> {
    let partial = destination.with_extension("part");
    let file = File::create(&partial)
        .map_err(|error| io_error("write an SDK package", &partial, &error))?;
    let mut writer = Hashing {
        inner: BufWriter::new(file),
        hasher: Sha256::new(),
    };
    image
        .copy_file_to(member.path, &mut writer)
        .map_err(|error| error.context(format!("member {}", member.path)))?;
    writer
        .flush()
        .map_err(|error| io_error("write an SDK package", &partial, &error))?;
    let digest = hex(&writer.hasher.finalize());

    if digest != member.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(ToolchainError::new(
            "The Windows SDK image on this computer is damaged.",
            format!(
                "sha256 mismatch for {}: expected {}, got {digest}",
                member.path, member.sha256
            ),
        ));
    }
    // The rename is the commit point, exactly as it is for a download: the member exists
    // under its real name only once its contents are proven.
    fs::rename(&partial, destination)
        .map_err(|error| io_error("finish writing an SDK package", destination, &error))
}

/// A writer that hashes what passes through it, so a 43 MB cabinet is never held in memory or
/// read back off the disk to be checked.
struct Hashing<W> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
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
    use crate::pinned::ISO_MEMBERS;

    /// Turn a real disc image into the eleven pinned packages, against the real pin.
    ///
    /// `#[ignore]`d because it needs the 1.45 GiB image. It works on a *copy*: harvesting
    /// deletes the image it read from, and a developer's own copy of an artifact that takes
    /// hours to download is not something a test may consume.
    ///
    /// ```bash
    /// CIV5VP_SDK_ISO=/path/to/GRMSDK_EN_DVD.iso \
    ///   cargo test --release -p civ5vp-toolchain --lib -- --ignored --nocapture harvest_a_real_image
    /// ```
    #[test]
    #[ignore = "needs a real Windows SDK disc image in CIV5VP_SDK_ISO"]
    fn harvest_a_real_image() {
        let Some(original) = std::env::var_os("CIV5VP_SDK_ISO") else {
            panic!("set CIV5VP_SDK_ISO to a Windows SDK 7.0 disc image");
        };
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("GRMSDK_EN_DVD.iso");
        fs::copy(&original, &image).unwrap();
        let into = dir.path().join("sdk");

        let freed = harvest_from_image(&image, ISO_MEMBERS, &into, &ProgressReporter::silent())
            .unwrap_or_else(|error| panic!("{}\n  detail: {}", error.message(), error.detail()));

        println!("freed {freed} bytes by dropping the image");
        assert!(!image.exists(), "the image should be gone");
        for file in ISO_MEMBERS.iter().flat_map(IsoMember::files) {
            let staged = into.join(file.cache_name());
            let metadata = fs::metadata(&staged)
                .unwrap_or_else(|_| panic!("{} should have been written", staged.display()));
            assert_eq!(metadata.len(), file.bytes, "{}", file.path);
            assert_eq!(hash_file(&staged).unwrap(), file.sha256, "{}", file.path);
        }
        // Harvesting again with the image gone is not something the caller does, but the
        // members it produced are exactly what a fetch would have produced.
        let staged = StagedMembers::new(into);
        assert!(staged.all_verified(ISO_MEMBERS));
    }
}
