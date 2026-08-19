//! Reading one file out of a Debian package.
//!
//! Used for exactly one artifact: the `libtinfo.so.5` the portable LLVM needs in order to
//! start (ADR-0005). A `.deb` is an `ar` archive holding three members - `debian-binary`, a
//! control tarball and a data tarball - and everything we want is one path inside the data
//! tarball.
//!
//! `ar` is parsed here rather than pulled in as a dependency: the format is a magic string
//! followed by 60-byte plain-text headers, which is less code than the justification for
//! adding a crate would be. The same call this project already made for the CLI,
//! the Steam `libraryfolders.vdf` parser and the settings format.
//!
//! The package is small - a third of a megabyte - so it is read whole rather than streamed.

use std::io::Read;
use std::path::Path;

use crate::error::{ToolchainError, io_error, stream_error};

/// `ar`'s file magic. Anything else is not an archive we can read.
const MAGIC: &[u8] = b"!<arch>\n";

/// Bytes in an `ar` member header, all of them plain text.
const HEADER_LEN: usize = 60;

/// Where the member size lives inside that header: decimal ASCII, space-padded.
const SIZE_FIELD: std::ops::Range<usize> = 48..58;

/// Where the member name lives: 16 bytes, space-padded, sometimes `/`-terminated.
const NAME_FIELD: std::ops::Range<usize> = 0..16;

/// Pull one file out of a `.deb`'s data tarball.
///
/// `wanted` is matched against the tar entry path with any leading `./` removed, so
/// `lib/x86_64-linux-gnu/libtinfo.so.5.9` matches the `./lib/…` the archive actually carries.
pub fn extract_from_data_tar(package: &Path, wanted: &str) -> Result<Vec<u8>, ToolchainError> {
    let bytes = std::fs::read(package)
        .map_err(|error| io_error("read the downloaded package", package, &error))?;

    let (name, data) = find_data_member(&bytes, package)?;
    let tar = decompress(name, data, package)?;
    find_in_tar(&tar, wanted, package)
}

/// The data tarball, and the name it went by - the name says how it is compressed.
fn find_data_member<'a>(
    bytes: &'a [u8],
    package: &Path,
) -> Result<(&'a str, &'a [u8]), ToolchainError> {
    let malformed = |detail: String| {
        ToolchainError::new(
            "A file the installer downloaded is not readable. Clear the installer's data \
             folder and try again.",
            detail,
        )
    };

    if !bytes.starts_with(MAGIC) {
        return Err(malformed(format!(
            "{} does not begin with the ar magic",
            package.display()
        )));
    }

    let mut at = MAGIC.len();
    while at + HEADER_LEN <= bytes.len() {
        let header = bytes.get(at..at + HEADER_LEN).ok_or_else(|| {
            malformed(format!(
                "truncated ar header at byte {at} of {}",
                package.display()
            ))
        })?;

        let name = header
            .get(NAME_FIELD)
            .map(|field| {
                String::from_utf8_lossy(field)
                    .trim()
                    .trim_end_matches('/')
                    .to_owned()
            })
            .unwrap_or_default();
        let size: usize = header
            .get(SIZE_FIELD)
            .map(|field| String::from_utf8_lossy(field).trim().to_owned())
            .unwrap_or_default()
            .parse()
            .map_err(|_| {
                malformed(format!(
                    "ar member {name:?} in {} has an unreadable size field",
                    package.display()
                ))
            })?;

        let start = at + HEADER_LEN;
        let data = bytes.get(start..start + size).ok_or_else(|| {
            malformed(format!(
                "ar member {name:?} claims {size} bytes but {} ends first",
                package.display()
            ))
        })?;

        if name.starts_with("data.tar") {
            // Borrowing the name out of `bytes` is not possible - it was trimmed into an owned
            // String - so hand back the slice of the header instead, which lives as long.
            let spelled = header
                .get(NAME_FIELD)
                .and_then(|field| std::str::from_utf8(field).ok())
                .unwrap_or("data.tar")
                .trim()
                .trim_end_matches('/');
            return Ok((spelled, data));
        }

        // Members are padded to an even offset.
        at = start + size + (size % 2);
    }

    Err(malformed(format!(
        "{} has no data.tar member",
        package.display()
    )))
}

/// Decompress the data tarball according to the extension its name carries.
///
/// Only `.xz` is supported, and that is deliberate rather than an oversight: the pinned
/// package is a Debian one precisely *because* Debian still uses xz, which `lzma-rs` already
/// reads for the LLVM tarball. Ubuntu's equivalent is zstd and would cost a dependency for one
/// 336 KB download (ADR-0005). A package that turns up compressed some other way is a changed
/// artifact, and saying so beats decoding it.
fn decompress(name: &str, data: &[u8], package: &Path) -> Result<Vec<u8>, ToolchainError> {
    if !name.ends_with(".xz") {
        return Err(ToolchainError::new(
            "A file the installer downloaded is not in the form it expected. Clear the \
             installer's data folder and try again.",
            format!(
                "{} carries {name}, but only xz-compressed data tarballs are pinned",
                package.display()
            ),
        ));
    }

    let mut out = Vec::new();
    let mut input = std::io::BufReader::new(data);
    lzma_rs::xz_decompress(&mut input, &mut out).map_err(|error| {
        ToolchainError::new(
            "A file the installer downloaded could not be unpacked. Clear the installer's \
             data folder and try again.",
            format!(
                "xz decode of {name} in {} failed: {error}",
                package.display()
            ),
        )
    })?;
    Ok(out)
}

fn find_in_tar(tar: &[u8], wanted: &str, package: &Path) -> Result<Vec<u8>, ToolchainError> {
    let mut archive = tar::Archive::new(tar);
    let entries = archive
        .entries()
        .map_err(|error| stream_error("read the package contents", &error))?;

    for entry in entries {
        let mut entry = entry.map_err(|error| stream_error("read a package entry", &error))?;
        let path = entry
            .path()
            .map_err(|error| stream_error("read a package entry name", &error))?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_owned();
        if path != wanted {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| stream_error("read a package entry's contents", &error))?;
        return Ok(bytes);
    }

    Err(ToolchainError::new(
        "A file the installer downloaded does not contain what it should. Clear the \
         installer's data folder and try again.",
        format!("{} has no {wanted} in its data tarball", package.display()),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use crate::test_fixtures::deb::package as synthetic_deb;

    fn write(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("package.deb");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn the_wanted_member_comes_back_byte_for_byte() {
        let contents = b"not really a shared library, but the bytes are the bytes";
        let (_dir, path) = write(&synthetic_deb(
            "lib/x86_64-linux-gnu/libtinfo.so.5.9",
            contents,
        ));

        let found = extract_from_data_tar(&path, "lib/x86_64-linux-gnu/libtinfo.so.5.9").unwrap();

        assert_eq!(found, contents);
    }

    /// The members before the one we want are skipped, padding included - an off-by-one in the
    /// even-offset rule would land the reader in the middle of a header.
    #[test]
    fn members_before_the_data_tarball_are_skipped() {
        let deb = synthetic_deb("lib/thing", b"x");
        assert!(deb.starts_with(MAGIC));
        let (_dir, path) = write(&deb);

        assert_eq!(extract_from_data_tar(&path, "lib/thing").unwrap(), b"x");
    }

    #[test]
    fn a_missing_member_is_reported_rather_than_guessed_at() {
        let (_dir, path) = write(&synthetic_deb("lib/present", b"x"));

        let error = extract_from_data_tar(&path, "lib/absent").unwrap_err();

        assert!(error.detail().contains("lib/absent"), "{}", error.detail());
        assert!(
            error
                .message()
                .contains("Clear the installer's data folder"),
            "the message should tell a player what to do: {}",
            error.message(),
        );
    }

    #[test]
    fn something_that_is_not_an_archive_is_refused() {
        let (_dir, path) = write(b"this is not an ar archive");

        let error = extract_from_data_tar(&path, "anything").unwrap_err();

        assert!(error.detail().contains("ar magic"), "{}", error.detail());
    }

    /// A truncated download must produce a sentence, not a panic - this module indexes into
    /// byte ranges taken from the file itself.
    #[test]
    fn a_truncated_package_is_refused_rather_than_panicking() {
        let full = synthetic_deb("lib/thing", b"some contents here");
        for cut in [MAGIC.len() + 1, MAGIC.len() + 40, full.len() / 2] {
            let (_dir, path) = write(&full[..cut]);
            assert!(
                extract_from_data_tar(&path, "lib/thing").is_err(),
                "a package cut at {cut} bytes should be refused",
            );
        }
    }
}
