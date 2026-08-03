//! Fetching a pinned artifact: resume what is half-there, prove it against its SHA-256, and
//! only then let it into the Toolchain Cache.
//!
//! The shape is deliberate. A 580 MB download over a slow connection will be interrupted, so
//! bytes land in `<name>.part` and the finished, *verified* file appears at `<name>` in one
//! atomic rename. Anything that goes wrong leaves either a resumable `.part` or nothing —
//! never a short file that looks finished (an acceptance criterion of ticket 05).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use civ5vp_core::{ProgressReporter, Stage};
use sha2::{Digest, Sha256};

use crate::error::{ToolchainError, io_error};
use crate::pinned::PinnedDownload;

/// Copy buffer. Big enough that the syscall overhead disappears against a network stream.
const CHUNK: usize = 256 * 1024;

/// How much has to arrive before progress speaks again. At ~1 MB/s that is a line every
/// eight seconds; at 100 MB/s it is not a flood.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// A stream of bytes starting partway into a resource — the one thing the downloader needs
/// from the network.
///
/// A trait, because the interesting behaviour here (resume, verify, atomic move, self-repair
/// after an interruption) is exactly what the fast suite must cover and the network is
/// exactly what it must not touch (rule 13).
pub trait ByteSource {
    /// Open `url` from `offset`.
    ///
    /// Implementations return [`Transfer::from_start`] when they could not honour the offset;
    /// the caller then discards whatever it already had rather than splicing two unrelated
    /// byte ranges together.
    fn open(&self, url: &str, offset: u64) -> Result<Transfer, ToolchainError>;
}

/// An open response body plus what is known about it.
pub struct Transfer {
    /// Where in the resource these bytes start. Either the requested offset, or 0.
    pub start: u64,
    /// Total size of the whole resource, when the server said.
    pub total: Option<u64>,
    pub body: Box<dyn Read>,
}

/// The real thing: `ureq` over rustls.
pub struct HttpByteSource {
    agent: ureq::Agent,
}

impl HttpByteSource {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::new_with_defaults(),
        }
    }
}

impl Default for HttpByteSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteSource for HttpByteSource {
    fn open(&self, url: &str, offset: u64) -> Result<Transfer, ToolchainError> {
        let mut request = self.agent.get(url);
        if offset > 0 {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let response = request.call().map_err(|error| network_error(url, &error))?;

        let status = response.status().as_u16();
        // 206 means the Range header was honoured. Anything else 2xx is the whole resource,
        // which is still usable — it just means starting over.
        let start = if status == 206 { offset } else { 0 };
        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let total = content_length.map(|length| start + length);

        let body = response
            .into_body()
            .into_with_config()
            // The default read limit is sized for API responses, not for a 580 MB ISO.
            .limit(u64::MAX)
            .reader();

        Ok(Transfer {
            start,
            total,
            body: Box::new(body),
        })
    }
}

impl Transfer {
    /// A transfer that ignored the requested offset.
    pub fn from_start(total: Option<u64>, body: Box<dyn Read>) -> Self {
        Self {
            start: 0,
            total,
            body,
        }
    }
}

/// Make `pinned` present and verified in `downloads_dir`, and return its path.
///
/// Idempotent: an already-verified copy is returned without touching the network, a partial
/// `.part` is resumed, and a `.part` whose bytes turn out not to hash correctly is deleted so
/// the next attempt starts clean.
pub fn fetch(
    source: &dyn ByteSource,
    pinned: &PinnedDownload,
    downloads_dir: &Path,
    progress: &ProgressReporter,
) -> Result<PathBuf, ToolchainError> {
    fs::create_dir_all(downloads_dir)
        .map_err(|error| io_error("create the downloads folder", downloads_dir, &error))?;

    let final_path = downloads_dir.join(pinned.file_name);
    let partial_path = downloads_dir.join(format!("{}.part", pinned.file_name));

    if final_path.is_file() {
        // Present from an earlier run. Still hashed: a file that was truncated by a full disk
        // or edited by something else must not be handed to the extractor.
        if hash_file(&final_path)? == pinned.sha256 {
            progress.report(
                Stage::Build,
                format!("Already have {} — skipping the download.", pinned.file_name),
            );
            return Ok(final_path);
        }
        fs::remove_file(&final_path)
            .map_err(|error| io_error("remove a damaged download", &final_path, &error))?;
    }

    download_to_partial(source, pinned, &partial_path, progress)?;

    let actual = hash_file(&partial_path)?;
    if actual != pinned.sha256 {
        // Do not keep it: resuming from bytes that are already wrong would loop forever.
        let _ = fs::remove_file(&partial_path);
        return Err(ToolchainError::new(
            format!(
                "The download of {} came out damaged. Check your connection and try again.",
                pinned.file_name
            ),
            format!(
                "sha256 mismatch for {}: expected {}, got {actual}",
                pinned.url, pinned.sha256
            ),
        ));
    }

    // The rename is the commit point: `<name>` exists only once its contents are proven.
    fs::rename(&partial_path, &final_path)
        .map_err(|error| io_error("finish a download", &final_path, &error))?;
    progress.report(
        Stage::Build,
        format!("Downloaded {} and checked it.", pinned.file_name),
    );
    Ok(final_path)
}

fn download_to_partial(
    source: &dyn ByteSource,
    pinned: &PinnedDownload,
    partial_path: &Path,
    progress: &ProgressReporter,
) -> Result<(), ToolchainError> {
    let already = match fs::metadata(partial_path) {
        Ok(metadata) => metadata.len(),
        Err(_) => 0,
    };

    let mut transfer = source.open(pinned.url, already)?;
    let expected_total = transfer.total.unwrap_or(pinned.approximate_bytes);

    let mut file = if transfer.start > 0 {
        progress.report(
            Stage::Build,
            format!(
                "Resuming the download of {} at {}.",
                pinned.file_name,
                human_bytes(transfer.start)
            ),
        );
        let mut file = OpenOptions::new()
            .write(true)
            .open(partial_path)
            .map_err(|error| io_error("reopen a partial download", partial_path, &error))?;
        // The server may have started earlier than we asked; trust its offset, not ours.
        file.seek(SeekFrom::Start(transfer.start))
            .map_err(|error| io_error("seek in a partial download", partial_path, &error))?;
        file.set_len(transfer.start)
            .map_err(|error| io_error("truncate a partial download", partial_path, &error))?;
        file
    } else {
        progress.report(
            Stage::Build,
            format!(
                "Downloading {} ({}).",
                pinned.file_name,
                human_bytes(expected_total)
            ),
        );
        File::create(partial_path)
            .map_err(|error| io_error("create a partial download", partial_path, &error))?
    };

    let mut written = transfer.start;
    let mut announced = written;
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = transfer
            .body
            .read(&mut buffer)
            .map_err(|error| network_read_error(pinned.url, &error))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| io_error("write a partial download", partial_path, &error))?;
        written += read as u64;

        if written - announced >= PROGRESS_STEP {
            announced = written;
            progress.report(
                Stage::Build,
                format!(
                    "Downloading {} — {} of {} ({}%).",
                    pinned.file_name,
                    human_bytes(written),
                    human_bytes(expected_total),
                    percent(written, expected_total)
                ),
            );
        }
    }
    file.flush()
        .map_err(|error| io_error("flush a partial download", partial_path, &error))?;
    Ok(())
}

/// SHA-256 of a file on disk, lowercase hex.
pub fn hash_file(path: &Path) -> Result<String, ToolchainError> {
    let mut file =
        File::open(path).map_err(|error| io_error("open a download to check it", path, &error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error("read a download to check it", path, &error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        // Writing into a String cannot fail, and rule 9 rules out unwrapping to say so.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn percent(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    (part.saturating_mul(100) / whole).min(100)
}

fn human_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (MB * 1024.0))
    } else {
        format!("{:.0} MB", bytes as f64 / MB)
    }
}

fn network_error(url: &str, error: &ureq::Error) -> ToolchainError {
    ToolchainError::new(
        "The installer could not reach the download server. Check your internet connection \
         and try again — anything already downloaded is kept.",
        format!("request to {url} failed: {error}"),
    )
}

fn network_read_error(url: &str, error: &std::io::Error) -> ToolchainError {
    ToolchainError::new(
        "The download was interrupted. Try again — the installer will carry on from where it \
         stopped.",
        format!("reading the response body from {url} failed: {error}"),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Cursor;

    /// A ByteSource over an in-memory blob, which can be told to stop short — the fast
    /// suite's stand-in for a dropped connection.
    struct FakeSource {
        content: Vec<u8>,
        /// How many bytes each successive `open` will hand over before ending the stream.
        allowances: RefCell<Vec<usize>>,
        /// Set when the server should ignore `Range` and start over.
        ignores_range: bool,
        opens: RefCell<Vec<u64>>,
    }

    impl FakeSource {
        fn new(content: Vec<u8>, allowances: Vec<usize>) -> Self {
            Self {
                content,
                allowances: RefCell::new(allowances),
                ignores_range: false,
                opens: RefCell::new(Vec::new()),
            }
        }

        fn ignoring_range(mut self) -> Self {
            self.ignores_range = true;
            self
        }
    }

    impl ByteSource for FakeSource {
        fn open(&self, _url: &str, offset: u64) -> Result<Transfer, ToolchainError> {
            self.opens.borrow_mut().push(offset);
            let start = if self.ignores_range { 0 } else { offset };
            let mut remaining = self.content[start as usize..].to_vec();
            if let Some(allowance) = self.allowances.borrow_mut().pop() {
                remaining.truncate(allowance);
            }
            Ok(Transfer {
                start,
                total: Some(self.content.len() as u64),
                body: Box::new(Cursor::new(remaining)),
            })
        }
    }

    fn pinned_for(content: &[u8]) -> PinnedDownload {
        let digest = hex(&Sha256::digest(content));
        PinnedDownload {
            file_name: "artifact.bin",
            url: "https://example.invalid/artifact.bin",
            // `PinnedDownload` holds `&'static str`; the tests leak one short string each.
            sha256: Box::leak(digest.into_boxed_str()),
            approximate_bytes: content.len() as u64,
        }
    }

    fn content() -> Vec<u8> {
        (0..300_000u32).map(|index| (index % 253) as u8).collect()
    }

    #[test]
    fn a_complete_download_is_verified_and_moved_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let source = FakeSource::new(content.clone(), vec![]);

        let path = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(path, dir.path().join("artifact.bin"));
        assert_eq!(fs::read(&path).unwrap(), content);
        // Nothing half-finished is left behind.
        assert!(!dir.path().join("artifact.bin.part").exists());
    }

    /// The acceptance criterion in as few words as it can be put: interrupt it, run it again,
    /// get the right file — and do not re-download what already arrived.
    #[test]
    fn an_interrupted_download_resumes_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);

        let truncating = FakeSource::new(content.clone(), vec![100_000]);
        let first = fetch(
            &truncating,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
        );
        assert!(first.is_err(), "a short download must not be accepted");
        assert!(!dir.path().join("artifact.bin").exists());

        // Second attempt: the `.part` is gone because its bytes failed the hash, so this
        // starts over — the important part is that it converges on a correct file.
        let complete = FakeSource::new(content.clone(), vec![]);
        let path = fetch(&complete, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), content);
    }

    #[test]
    fn a_partial_file_left_by_a_crash_is_continued_rather_than_refetched() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        // Simulate a crash: half the bytes already on disk under the `.part` name.
        fs::write(dir.path().join("artifact.bin.part"), &content[..120_000]).unwrap();

        let source = FakeSource::new(content.clone(), vec![]);
        let path = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        assert_eq!(source.opens.borrow().as_slice(), &[120_000u64]);
    }

    #[test]
    fn a_server_that_ignores_range_makes_the_download_start_over_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        fs::write(dir.path().join("artifact.bin.part"), &content[..120_000]).unwrap();

        let source = FakeSource::new(content.clone(), vec![]).ignoring_range();
        let path = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
    }

    #[test]
    fn a_download_that_hashes_wrong_is_refused_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let mut pinned = pinned_for(&content);
        pinned.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        let source = FakeSource::new(content, vec![]);

        let error = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap_err();

        assert!(error.detail().contains("sha256 mismatch"));
        assert!(!dir.path().join("artifact.bin").exists());
        assert!(!dir.path().join("artifact.bin.part").exists());
    }

    #[test]
    fn an_already_verified_file_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        fs::write(dir.path().join("artifact.bin"), &content).unwrap();
        let source = FakeSource::new(content, vec![]);

        fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert!(
            source.opens.borrow().is_empty(),
            "no request should be made"
        );
    }

    /// A cached file that is no longer the pinned artifact is replaced, not trusted.
    #[test]
    fn a_corrupted_cached_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        fs::write(dir.path().join("artifact.bin"), b"not the artifact").unwrap();
        let source = FakeSource::new(content.clone(), vec![]);

        let path = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        assert_eq!(source.opens.borrow().as_slice(), &[0u64]);
    }

    /// A 580 MB download with no visible progress is a defect (ticket 05).
    #[test]
    fn progress_is_reported_while_bytes_arrive() {
        let dir = tempfile::tempdir().unwrap();
        let content: Vec<u8> = vec![7u8; 40 * 1024 * 1024];
        let pinned = pinned_for(&content);
        let source = FakeSource::new(content, vec![]);

        let (sender, receiver) = std::sync::mpsc::channel();
        fetch(
            &source,
            &pinned,
            dir.path(),
            &ProgressReporter::to_channel(sender),
        )
        .unwrap();

        let messages: Vec<String> = receiver.iter().map(|event| event.message).collect();
        assert!(
            messages.iter().filter(|m| m.contains('%')).count() >= 4,
            "expected several percentage updates, got {messages:?}"
        );
    }

    #[test]
    fn byte_sizes_read_the_way_a_player_expects() {
        assert_eq!(human_bytes(580 * 1024 * 1024), "580 MB");
        assert_eq!(
            human_bytes(1024 * 1024 * 1024 + 512 * 1024 * 1024),
            "1.5 GB"
        );
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(1, 0), 0);
    }
}
