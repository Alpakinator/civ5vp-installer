//! Getting named members out of a CAB.
//!
//! Thin, because the `cab` crate does the work — MSZIP through `flate2` and LZX through the
//! pure-Rust `lzxd`. What this module adds is the two things the bootstrap needs on top:
//! errors phrased for a player, and a clear failure for Quantum, the one compression the
//! crate does not implement.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use crate::error::{ToolchainError, io_error, stream_error};

/// An open cabinet on disk.
pub struct Cabinet {
    inner: cab::Cabinet<BufReader<File>>,
    label: String,
}

impl Cabinet {
    pub fn open(path: &Path) -> Result<Self, ToolchainError> {
        let file = File::open(path).map_err(|error| io_error("open a cabinet", path, &error))?;
        let inner = cab::Cabinet::new(BufReader::new(file)).map_err(|error| {
            ToolchainError::new(
                "The Windows SDK download could not be unpacked. Clear the installer's data \
                 folder and try again.",
                format!("{} is not a readable cabinet: {error}", path.display()),
            )
        })?;
        Ok(Self {
            inner,
            label: path.display().to_string(),
        })
    }

    /// Whether this cabinet holds a member under that exact name.
    pub fn contains(&self, name: &str) -> bool {
        self.inner.get_file_entry(name).is_some()
    }

    /// Decompress one member into `out`.
    ///
    /// Names are matched exactly. The MSI's `File` key is the CAB-internal name, so an
    /// inexact match here would mean the layout mapping was wrong, and quietly picking a
    /// near-miss would put the wrong bytes on disk.
    pub fn extract(&mut self, name: &str, out: &mut impl Write) -> Result<u64, ToolchainError> {
        let mut reader = self.inner.read_file(name).map_err(|error| {
            ToolchainError::new(
                "The Windows SDK download could not be unpacked. Clear the installer's data \
                 folder and try again.",
                format!("{name} could not be read from {}: {error}", self.label),
            )
        })?;

        let mut buffer = vec![0u8; 256 * 1024];
        let mut written = 0u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| decompression_error(name, &self.label, &error))?;
            if read == 0 {
                break;
            }
            out.write_all(&buffer[..read])
                .map_err(|error| stream_error("write an extracted file", &error))?;
            written += read as u64;
        }
        Ok(written)
    }
}

/// The `cab` crate reports unsupported Quantum compression as an ordinary IO error, which
/// would otherwise reach the user as "invalid data". If the SDK cabinets ever turn out to use
/// it, this is the sentence that has to change into a plan.
fn decompression_error(name: &str, label: &str, error: &std::io::Error) -> ToolchainError {
    let message = if error.to_string().contains("Quantum") {
        "This Windows SDK download uses a compression the installer cannot read. Please \
         report this — the installer cannot continue."
    } else {
        "The Windows SDK download could not be unpacked. Clear the installer's data folder \
         and try again."
    };
    ToolchainError::new(
        message,
        format!("decompressing {name} from {label}: {error}"),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::test_fixtures::cabinet::build;

    fn write_cabinet(dir: &Path, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = dir.join("cab1.cab");
        std::fs::write(&path, build(files)).unwrap();
        path
    }

    #[test]
    fn members_come_back_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        // Compressible and incompressible payloads, both bigger than one MSZIP block would
        // be if the crate ever changed how it splits them.
        let big: Vec<u8> = (0..70_000u32).map(|index| (index % 251) as u8).collect();
        let path = write_cabinet(
            dir.path(),
            &[("windows.h", b"#include <winnt.h>\n"), ("big.lib", &big)],
        );

        let mut cabinet = Cabinet::open(&path).unwrap();
        let mut header = Vec::new();
        cabinet.extract("windows.h", &mut header).unwrap();
        let mut lib = Vec::new();
        let written = cabinet.extract("big.lib", &mut lib).unwrap();

        assert_eq!(header, b"#include <winnt.h>\n");
        assert_eq!(lib, big);
        assert_eq!(written, big.len() as u64);
    }

    #[test]
    fn membership_is_answered_without_decompressing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_cabinet(dir.path(), &[("file1.h", b"x")]);
        let cabinet = Cabinet::open(&path).unwrap();

        assert!(cabinet.contains("file1.h"));
        assert!(!cabinet.contains("file2.h"));
    }

    #[test]
    fn a_missing_member_names_itself_in_the_log_but_not_in_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_cabinet(dir.path(), &[("file1.h", b"x")]);
        let mut cabinet = Cabinet::open(&path).unwrap();

        let error = cabinet.extract("file2.h", &mut Vec::new()).unwrap_err();

        assert!(error.detail().contains("file2.h"));
        assert!(!error.message().contains("file2.h"));
    }

    #[test]
    fn something_that_is_not_a_cabinet_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cab1.cab");
        std::fs::write(&path, b"not a cabinet at all").unwrap();

        let Err(error) = Cabinet::open(&path) else {
            panic!("a file that is not a cabinet must not open");
        };
        assert!(error.detail().contains("not a readable cabinet"));
    }
}
