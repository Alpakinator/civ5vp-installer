//! Which filesystem the downloaded disc image actually uses, and one API over both.
//!
//! The pinned Windows SDK image is UDF; its ISO9660 side holds a `README.TXT` and nothing
//! else (see [`crate::udf`]). `docs/pinned-artifacts.md` and ADR-0001 both say "ISO9660", so
//! both readers are kept: the UDF one because it is what extracts the real artifact, the
//! ISO9660 one because it is what the documents describe and what a plainer image would need.
//!
//! The choice is made by probing, not by configuration. An image that carries a UDF anchor is
//! read as UDF; anything else is read as ISO9660.

use std::io::{Read, Seek, Write};

use crate::error::ToolchainError;
use crate::iso9660::Iso9660;
use crate::udf::{self, Udf};

/// One entry in a directory, whichever filesystem it came from. Diagnostics only — the
/// bootstrap navigates by the paths in `docs/pinned-artifacts.md`, never by listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscEntry {
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
}

pub enum Disc<R> {
    Udf(Box<Udf<R>>),
    Iso9660(Box<Iso9660<R>>),
}

impl<R: Read + Seek> Disc<R> {
    /// Probe for a UDF anchor, then open the image as whichever filesystem it turns out to be.
    pub fn open(mut reader: R) -> Result<Self, ToolchainError> {
        if udf::has_anchor(&mut reader) {
            return Udf::open(reader).map(|volume| Self::Udf(Box::new(volume)));
        }
        Iso9660::open(reader).map(|volume| Self::Iso9660(Box::new(volume)))
    }

    /// What this image turned out to be. Goes in the log, because "which filesystem?" is the
    /// first question when an extraction finds nothing.
    pub fn format(&self) -> &'static str {
        match self {
            Self::Udf(_) => "UDF",
            Self::Iso9660(_) => "ISO9660",
        }
    }

    pub fn read_dir(&mut self, path: &str) -> Result<Vec<DiscEntry>, ToolchainError> {
        match self {
            Self::Udf(volume) => Ok(volume
                .read_dir(path)?
                .into_iter()
                .map(|entry| DiscEntry {
                    name: entry.name,
                    is_directory: entry.is_directory,
                    size: entry.size,
                })
                .collect()),
            Self::Iso9660(volume) => Ok(volume
                .read_dir(path)?
                .into_iter()
                .map(|entry| DiscEntry {
                    name: entry.name,
                    is_directory: entry.is_directory,
                    size: entry.size,
                })
                .collect()),
        }
    }

    /// Where `path`'s bytes lie in the image: absolute `(offset, length)` pairs, in order.
    /// Empty when the filesystem stores the data inside the entry itself. Test-only — see
    /// [`crate::udf::Udf::extents`].
    #[cfg(test)]
    pub fn extents(&mut self, path: &str) -> Result<Vec<(u64, u64)>, ToolchainError> {
        match self {
            Self::Udf(volume) => volume.extents(path),
            Self::Iso9660(volume) => volume.extents(path),
        }
    }

    /// Read a whole member into memory.
    ///
    /// Test-only: the bootstrap streams members out with `copy_file_to`, so a 43 MB cabinet
    /// is never held in memory. This is what the image-inspection tools read with.
    #[cfg(test)]
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, ToolchainError> {
        match self {
            Self::Udf(volume) => volume.read_file(path),
            Self::Iso9660(volume) => volume.read_file(path),
        }
    }

    pub fn copy_file_to(
        &mut self,
        path: &str,
        out: &mut impl Write,
    ) -> Result<u64, ToolchainError> {
        match self {
            Self::Udf(volume) => volume.copy_file_to(path, out),
            Self::Iso9660(volume) => volume.copy_file_to(path, out),
        }
    }

    pub fn contains(&mut self, path: &str) -> bool {
        match self {
            Self::Udf(volume) => volume.contains(path),
            Self::Iso9660(volume) => volume.contains(path),
        }
    }
}

pub fn open(
    path: &std::path::Path,
) -> Result<Disc<std::io::BufReader<std::fs::File>>, ToolchainError> {
    let file = std::fs::File::open(path)
        .map_err(|error| crate::error::io_error("open the SDK disc image", path, &error))?;
    Disc::open(std::io::BufReader::new(file))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_fixtures::{iso::IsoBuilder, udf::UdfBuilder};
    use std::io::Cursor;

    #[test]
    fn a_udf_image_is_recognised_and_read_as_udf() {
        let image = UdfBuilder::new()
            .file("Setup/WinSDK/WinSDK_x86.msi", b"msi".to_vec())
            .build();
        let mut disc = Disc::open(Cursor::new(image)).unwrap();

        assert_eq!(disc.format(), "UDF");
        assert_eq!(
            disc.read_file("Setup/WinSDK/WinSDK_x86.msi").unwrap(),
            b"msi"
        );
    }

    #[test]
    fn an_iso9660_image_is_recognised_and_read_as_iso9660() {
        let image = IsoBuilder::new()
            .file("Setup/WinSDK/WinSDK_x86.msi", b"msi".to_vec())
            .build();
        let mut disc = Disc::open(Cursor::new(image)).unwrap();

        assert_eq!(disc.format(), "ISO9660");
        assert_eq!(
            disc.read_file("Setup/WinSDK/WinSDK_x86.msi").unwrap(),
            b"msi"
        );
    }

    /// The real image's shape: a UDF volume whose ISO9660 side is a stub. Reading it as
    /// ISO9660 finds a README and nothing else, which is exactly the failure the probe exists
    /// to avoid.
    #[test]
    fn a_hybrid_image_is_read_through_its_udf_side() {
        let mut image = UdfBuilder::new()
            .file("Setup/WinSDK/WinSDK_x86.msi", b"msi".to_vec())
            .build();
        let stub = IsoBuilder::new()
            .file(
                "README.TXT",
                b"This disc contains a \"UDF\" file system.".to_vec(),
            )
            .build();
        // The ISO9660 descriptors live at sectors 16 onwards; the UDF anchor is at 256, so
        // the two overlay without colliding.
        image[16 * 2048..stub.len()].copy_from_slice(&stub[16 * 2048..]);

        let mut disc = Disc::open(Cursor::new(image)).unwrap();

        assert_eq!(disc.format(), "UDF");
        assert!(disc.contains("Setup/WinSDK/WinSDK_x86.msi"));
    }
}
