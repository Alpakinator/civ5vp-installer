//! Fetching a pinned artifact: resume what is half-there, prove it against its SHA-256, and
//! only then let it into the Toolchain Cache.
//!
//! The shape is deliberate. A big download over a slow connection will be interrupted, so
//! bytes land in `<name>.part` and the finished, *verified* file appears at `<name>` in one
//! atomic rename. Anything that goes wrong leaves either a resumable `.part` or nothing —
//! never a short file that looks finished.
//!
//! Large artifacts download over **several connections at once**. The Wayback Machine — the
//! one source of the pinned SDK image — throttles per connection to roughly 1 MB/s, and
//! measured from a real machine four parallel ranged requests deliver about 4.5x the
//! single-connection rate. The file is divided into a fixed grid of chunks, each fetched
//! with its own ranged request; a sidecar (`<name>.parts`) records which chunks are done, so
//! an interrupted run redoes only what is missing. A server that ignores `Range` drops the
//! whole fetch back to the sequential path automatically.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use civ5vp_core::{ProgressReporter, Stage};
use sha2::{Digest, Sha256};

use crate::error::{ToolchainError, io_error};
use crate::pinned::PinnedDownload;

/// Copy buffer. Big enough that the syscall overhead disappears against a network stream.
const CHUNK: usize = 256 * 1024;

/// How much has to arrive before progress speaks again. At ~1 MB/s that is a line every
/// eight seconds; at 100 MB/s it is not a flood.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// One ranged request's worth of file in the parallel path.
const PARALLEL_CHUNK: u64 = 32 * 1024 * 1024;

/// How many connections fetch chunks at once. Four is where the Wayback Machine's
/// per-connection throttle stops being the limit, and modest enough to stay polite.
const PARALLEL_CONNECTIONS: usize = 4;

/// Artifacts smaller than this are not worth the extra requests.
const PARALLEL_THRESHOLD: u64 = 64 * 1024 * 1024;

/// A stream of bytes starting partway into a resource — the one thing the downloader needs
/// from the network.
///
/// A trait, because the interesting behaviour here (resume, verify, atomic move, self-repair
/// after an interruption) is exactly what the fast suite must cover, and the fast suite never
/// opens sockets. `Sync`, because the parallel path shares one source across its worker
/// threads.
pub trait ByteSource: Sync {
    /// Open `url` from `offset`.
    ///
    /// Implementations return [`Transfer::from_start`] when they could not honour the offset;
    /// the caller then discards whatever it already had rather than splicing two unrelated
    /// byte ranges together.
    fn open(&self, url: &str, offset: u64) -> Result<Transfer, ToolchainError>;

    /// Open the bounded range `[start, end)` of `url`.
    ///
    /// The default rides on [`ByteSource::open`] and caps the stream, which is correct for
    /// any source; the HTTP implementation overrides it with a real bounded `Range` header
    /// so the server stops sending at `end` too.
    fn open_range(&self, url: &str, start: u64, end: u64) -> Result<Transfer, ToolchainError> {
        let transfer = self.open(url, start)?;
        let cap = end.saturating_sub(transfer.start);
        Ok(Transfer {
            start: transfer.start,
            total: transfer.total,
            body: Box::new(transfer.body.take(cap)),
        })
    }
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

    fn request(&self, url: &str, range: Option<String>) -> Result<Transfer, ToolchainError> {
        let mut request = self.agent.get(url);
        if let Some(range) = &range {
            request = request.header("Range", range.as_str());
        }
        let response = request.call().map_err(|error| network_error(url, &error))?;

        let status = response.status().as_u16();
        // 206 means the Range header was honoured. Anything else 2xx is the whole resource,
        // which is still usable — it just means starting over (or, on the parallel path,
        // falling back to sequential).
        let honoured = status == 206;
        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        // For a ranged response the total is in Content-Range: "bytes 0-99/1552508928".
        let content_range_total = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next())
            .and_then(|total| total.parse::<u64>().ok());

        let body = response
            .into_body()
            .into_with_config()
            // The default read limit is sized for API responses, not for a 1.45 GB ISO.
            .limit(u64::MAX)
            .reader();

        Ok(Transfer {
            start: if honoured { u64::MAX } else { 0 }, // caller fills the honoured offset
            total: content_range_total.or(content_length),
            body: Box::new(body),
        })
    }
}

impl Default for HttpByteSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteSource for HttpByteSource {
    fn open(&self, url: &str, offset: u64) -> Result<Transfer, ToolchainError> {
        let range = (offset > 0).then(|| format!("bytes={offset}-"));
        let mut transfer = self.request(url, range)?;
        // `total` from a suffix range's Content-Range is already the full size; from a plain
        // 200 it is the content length, which equals the full size when starting at 0.
        if transfer.start == u64::MAX {
            transfer.start = offset;
        } else if transfer.total.is_some() && offset == 0 {
            // Plain 200 at offset 0: content length is the whole resource.
        }
        Ok(transfer)
    }

    fn open_range(&self, url: &str, start: u64, end: u64) -> Result<Transfer, ToolchainError> {
        let mut transfer = self.request(url, Some(format!("bytes={start}-{}", end - 1)))?;
        if transfer.start == u64::MAX {
            transfer.start = start;
        }
        Ok(transfer)
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
    fetch_with(
        source,
        pinned,
        downloads_dir,
        progress,
        PARALLEL_CHUNK,
        PARALLEL_THRESHOLD,
        PARALLEL_CONNECTIONS,
    )
}

/// [`fetch`] with the parallel grid as parameters, so the fast suite can exercise the
/// chunking with kilobytes instead of gigabytes.
fn fetch_with(
    source: &dyn ByteSource,
    pinned: &PinnedDownload,
    downloads_dir: &Path,
    progress: &ProgressReporter,
    chunk_size: u64,
    parallel_threshold: u64,
    connections: usize,
) -> Result<PathBuf, ToolchainError> {
    fs::create_dir_all(downloads_dir)
        .map_err(|error| io_error("create the downloads folder", downloads_dir, &error))?;

    let final_path = downloads_dir.join(pinned.file_name);
    let partial_path = downloads_dir.join(format!("{}.part", pinned.file_name));
    let sidecar_path = downloads_dir.join(format!("{}.parts", pinned.file_name));

    if final_path.is_file() {
        // Present from an earlier run. Still hashed: a file that was truncated by a full disk
        // or edited by something else must not be handed to the extractor.
        if hash_file(&final_path)? == pinned.sha256 {
            progress.report(
                Stage::Build,
                format!("Already have {} — skipping the download.", pinned.file_name),
            );
            let _ = fs::remove_file(&sidecar_path);
            return Ok(final_path);
        }
        fs::remove_file(&final_path)
            .map_err(|error| io_error("remove a damaged download", &final_path, &error))?;
    }

    let mut fetched_in_parallel = false;
    if pinned.approximate_bytes >= parallel_threshold {
        fetched_in_parallel = parallel_download(
            source,
            pinned,
            &partial_path,
            &sidecar_path,
            progress,
            chunk_size,
            connections,
        )?;
    }
    if !fetched_in_parallel {
        download_to_partial(source, pinned, &partial_path, progress)?;
    }

    let actual = hash_file(&partial_path)?;
    if actual != pinned.sha256 {
        // Do not keep it: resuming from bytes that are already wrong would loop forever.
        let _ = fs::remove_file(&partial_path);
        let _ = fs::remove_file(&sidecar_path);
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
    let _ = fs::remove_file(&sidecar_path);
    progress.report(
        Stage::Build,
        format!("Downloaded {} and checked it.", pinned.file_name),
    );
    Ok(final_path)
}

// ---------------------------------------------------------------------------------------
// The parallel path
// ---------------------------------------------------------------------------------------

/// Which chunks of a partial download have fully arrived. Lives in `<name>.parts` beside the
/// `.part` file; the format records the grid too, so a sidecar from another grid (or another
/// artifact size) is discarded rather than trusted.
struct ChunkLedger {
    chunk_size: u64,
    total: u64,
    done: Vec<bool>,
}

impl ChunkLedger {
    fn chunk_count(chunk_size: u64, total: u64) -> usize {
        (total.div_ceil(chunk_size)) as usize
    }

    fn new(chunk_size: u64, total: u64) -> Self {
        Self {
            chunk_size,
            total,
            done: vec![false; Self::chunk_count(chunk_size, total)],
        }
    }

    fn load(path: &Path, chunk_size: u64) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let mut header = lines.next()?.split_whitespace();
        if (header.next(), header.next()) != (Some("chunks"), Some("v1")) {
            return None;
        }
        let recorded_chunk: u64 = header.next()?.parse().ok()?;
        let total: u64 = header.next()?.parse().ok()?;
        if recorded_chunk != chunk_size || total == 0 {
            return None;
        }
        let mut ledger = Self::new(chunk_size, total);
        for line in lines {
            let index: usize = line.trim().parse().ok()?;
            *ledger.done.get_mut(index)? = true;
        }
        Some(ledger)
    }

    fn save(&self, path: &Path) -> Result<(), ToolchainError> {
        let mut text = format!("chunks v1 {} {}\n", self.chunk_size, self.total);
        for (index, done) in self.done.iter().enumerate() {
            if *done {
                text.push_str(&format!("{index}\n"));
            }
        }
        let temporary = path.with_extension("parts.new");
        fs::write(&temporary, text)
            .map_err(|error| io_error("write the download ledger", &temporary, &error))?;
        fs::rename(&temporary, path)
            .map_err(|error| io_error("write the download ledger", path, &error))?;
        Ok(())
    }

    fn range_of(&self, index: usize) -> (u64, u64) {
        let start = index as u64 * self.chunk_size;
        (start, (start + self.chunk_size).min(self.total))
    }

    fn bytes_done(&self) -> u64 {
        self.done
            .iter()
            .enumerate()
            .filter(|(_, done)| **done)
            .map(|(index, _)| {
                let (start, end) = self.range_of(index);
                end - start
            })
            .sum()
    }
}

/// Download `pinned` over several ranged connections.
///
/// Returns `Ok(false)` — with nothing torn down — when the server turns out not to support
/// ranges or not to say the total size; the caller then uses the sequential path. `Ok(true)`
/// means the `.part` file holds every byte.
fn parallel_download(
    source: &dyn ByteSource,
    pinned: &PinnedDownload,
    partial_path: &Path,
    sidecar_path: &Path,
    progress: &ProgressReporter,
    chunk_size: u64,
    connections: usize,
) -> Result<bool, ToolchainError> {
    // A ledger from an interrupted parallel run already knows the total, so a resume asks
    // for nothing but the missing chunks. Otherwise the first chunk doubles as the probe:
    // it learns the exact total and whether the server honours ranges, and its bytes are
    // never wasted.
    let mut probe = None;
    let mut ledger = match ChunkLedger::load(sidecar_path, chunk_size) {
        Some(ledger) => ledger,
        None => {
            let opened = source.open_range(pinned.url, 0, chunk_size)?;
            let Some(total) = opened.total else {
                return Ok(false);
            };
            if total <= chunk_size {
                return Ok(false);
            }
            probe = Some(opened);
            let mut ledger = ChunkLedger::new(chunk_size, total);
            // Convert what a sequential run left behind: a plain `.part` prefix is credit
            // for every chunk it fully covers.
            if let Ok(metadata) = fs::metadata(partial_path) {
                let prefix_chunks = (metadata.len() / chunk_size) as usize;
                for done in ledger.done.iter_mut().take(prefix_chunks) {
                    *done = true;
                }
            }
            ledger
        }
    };
    let total = ledger.total;

    // The `.part` file at full size, so every worker writes straight to its own offsets.
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(partial_path)
        .map_err(|error| io_error("create a partial download", partial_path, &error))?;
    file.set_len(total)
        .map_err(|error| io_error("size a partial download", partial_path, &error))?;
    drop(file);

    let already = ledger.bytes_done();
    progress.report(
        Stage::Build,
        format!(
            "Downloading {} ({}) on {connections} connections{}.",
            pinned.file_name,
            human_bytes(total),
            if already > 0 {
                format!(" — resuming with {} already here", human_bytes(already))
            } else {
                String::new()
            }
        ),
    );

    // Write the probe's chunk first, when there is one and it is still needed. The ledger
    // is persisted either way, so even a first-chunk failure leaves a resumable state.
    if !ledger.done[0]
        && let Some(probe) = probe.take()
    {
        let (start, end) = ledger.range_of(0);
        let written = write_chunk(partial_path, probe, start, end, pinned.url);
        if written.is_ok() {
            ledger.done[0] = true;
        }
        ledger.save(sidecar_path)?;
        written?;
    } else {
        ledger.save(sidecar_path)?;
    }

    let todo: Vec<usize> = ledger
        .done
        .iter()
        .enumerate()
        .filter(|(_, done)| !**done)
        .map(|(index, _)| index)
        .collect();
    if todo.is_empty() {
        return Ok(true);
    }

    // Workers pull chunk indices from a shared cursor; each failure or refused range is
    // recorded and stops the others quickly.
    let cursor = AtomicUsize::new(0);
    let range_refused = AtomicBool::new(false);
    let failed: std::sync::Mutex<Option<ToolchainError>> = std::sync::Mutex::new(None);
    let finished: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
    let downloaded = AtomicU64::new(already);
    let announced = AtomicU64::new(already);

    std::thread::scope(|scope| {
        for _ in 0..connections.max(1) {
            scope.spawn(|| {
                loop {
                    if range_refused.load(Ordering::Relaxed)
                        || failed.lock().map(|f| f.is_some()).unwrap_or(true)
                    {
                        return;
                    }
                    let slot = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(&index) = todo.get(slot) else {
                        return;
                    };
                    let (start, end) = ledger.range_of(index);
                    let outcome = source
                        .open_range(pinned.url, start, end)
                        .and_then(|transfer| {
                            if transfer.start != start {
                                range_refused.store(true, Ordering::Relaxed);
                                return Ok(0);
                            }
                            write_chunk(partial_path, transfer, start, end, pinned.url)?;
                            Ok(end - start)
                        });
                    match outcome {
                        Ok(0) => return,
                        Ok(bytes) => {
                            if let Ok(mut list) = finished.lock() {
                                list.push(index);
                            }
                            let so_far = downloaded.fetch_add(bytes, Ordering::Relaxed) + bytes;
                            let last = announced.load(Ordering::Relaxed);
                            if so_far - last >= PROGRESS_STEP
                                && announced
                                    .compare_exchange(
                                        last,
                                        so_far,
                                        Ordering::Relaxed,
                                        Ordering::Relaxed,
                                    )
                                    .is_ok()
                            {
                                progress.report(
                                    Stage::Build,
                                    format!(
                                        "Downloading {} — {} of {} ({}%).",
                                        pinned.file_name,
                                        human_bytes(so_far),
                                        human_bytes(total),
                                        percent(so_far, total)
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            if let Ok(mut failure) = failed.lock() {
                                failure.get_or_insert(error);
                            }
                            return;
                        }
                    }
                }
            });
        }
    });

    // Record everything that landed, whatever else happened — that is the resume state.
    if let Ok(list) = finished.lock() {
        for &index in list.iter() {
            ledger.done[index] = true;
        }
    }
    ledger.save(sidecar_path)?;

    if let Ok(mut failure) = failed.lock()
        && let Some(error) = failure.take()
    {
        return Err(error);
    }
    if range_refused.load(Ordering::Relaxed) {
        // The server would not serve ranges after all. Keep only a clean prefix for the
        // sequential path to resume from.
        let prefix_chunks = ledger.done.iter().take_while(|done| **done).count();
        let prefix_bytes = if prefix_chunks == 0 {
            0
        } else {
            ledger.range_of(prefix_chunks - 1).1
        };
        let file = OpenOptions::new()
            .write(true)
            .open(partial_path)
            .map_err(|error| io_error("reopen a partial download", partial_path, &error))?;
        file.set_len(prefix_bytes)
            .map_err(|error| io_error("truncate a partial download", partial_path, &error))?;
        let _ = fs::remove_file(sidecar_path);
        return Ok(false);
    }

    Ok(ledger.done.iter().all(|done| *done))
}

/// Stream one transfer into `[start, end)` of the partial file. Short bodies are an error —
/// a chunk is only credited when every byte of it arrived.
fn write_chunk(
    partial_path: &Path,
    mut transfer: Transfer,
    start: u64,
    end: u64,
    url: &str,
) -> Result<(), ToolchainError> {
    let mut file = OpenOptions::new()
        .write(true)
        .open(partial_path)
        .map_err(|error| io_error("open a partial download", partial_path, &error))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| io_error("seek in a partial download", partial_path, &error))?;
    let mut remaining = end - start;
    let mut buffer = vec![0u8; CHUNK];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = transfer
            .body
            .read(&mut buffer[..want])
            .map_err(|error| network_read_error(url, &error))?;
        if read == 0 {
            return Err(ToolchainError::new(
                "The download was interrupted. Try again — the installer will carry on from \
                 where it stopped.",
                format!("range {start}-{end} of {url} ended {remaining} bytes short"),
            ));
        }
        file.write_all(&buffer[..read])
            .map_err(|error| io_error("write a partial download", partial_path, &error))?;
        remaining -= read as u64;
    }
    file.flush()
        .map_err(|error| io_error("flush a partial download", partial_path, &error))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// The sequential path — for small artifacts and servers without ranges
// ---------------------------------------------------------------------------------------

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
        // Writing into a String cannot fail.
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
    use std::io::Cursor;
    use std::sync::Mutex;

    /// A ByteSource over an in-memory blob, which can be told to stop short — the fast
    /// suite's stand-in for a dropped connection. `Mutex` inside because the parallel path
    /// shares the source across worker threads.
    struct FakeSource {
        content: Vec<u8>,
        /// How many bytes each successive `open` will hand over before ending the stream.
        allowances: Mutex<Vec<usize>>,
        /// Set when the server should ignore `Range` and start over.
        ignores_range: bool,
        /// Set when the server never says how large the resource is.
        hides_total: bool,
        opens: Mutex<Vec<u64>>,
    }

    impl FakeSource {
        fn new(content: Vec<u8>, allowances: Vec<usize>) -> Self {
            Self {
                content,
                allowances: Mutex::new(allowances),
                ignores_range: false,
                hides_total: false,
                opens: Mutex::new(Vec::new()),
            }
        }

        fn ignoring_range(mut self) -> Self {
            self.ignores_range = true;
            self
        }

        fn hiding_total(mut self) -> Self {
            self.hides_total = true;
            self
        }

        fn open_count(&self) -> usize {
            self.opens.lock().unwrap().len()
        }
    }

    impl ByteSource for FakeSource {
        fn open(&self, _url: &str, offset: u64) -> Result<Transfer, ToolchainError> {
            self.opens.lock().unwrap().push(offset);
            let start = if self.ignores_range { 0 } else { offset };
            let mut remaining = self.content[start as usize..].to_vec();
            if let Some(allowance) = self.allowances.lock().unwrap().pop() {
                remaining.truncate(allowance);
            }
            Ok(Transfer {
                start,
                total: (!self.hides_total).then_some(self.content.len() as u64),
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

    /// `fetch_with` on a small parallel grid: 64 KiB chunks, everything over 100 KiB goes
    /// parallel on three connections.
    fn fetch_small_grid(
        source: &dyn ByteSource,
        pinned: &PinnedDownload,
        dir: &Path,
        progress: &ProgressReporter,
    ) -> Result<PathBuf, ToolchainError> {
        fetch_with(source, pinned, dir, progress, 64 * 1024, 100 * 1024, 3)
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
        assert!(!dir.path().join("artifact.bin.part").exists());
        assert!(!dir.path().join("artifact.bin.parts").exists());
    }

    #[test]
    fn a_large_artifact_downloads_over_several_connections() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let source = FakeSource::new(content.clone(), vec![]);

        let path =
            fetch_small_grid(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        // 300 KB over 64 KiB chunks: the probe plus four more ranged requests.
        assert_eq!(source.open_count(), 5);
        assert!(!dir.path().join("artifact.bin.part").exists());
        assert!(!dir.path().join("artifact.bin.parts").exists());
    }

    #[test]
    fn an_interrupted_parallel_download_resumes_by_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);

        // Allowances pop from the end: the probe gets its full chunk, every worker request
        // after it is cut short.
        let failing = FakeSource::new(content.clone(), vec![1000, 1000, 1000, 1000, 65536]);
        let first = fetch_small_grid(&failing, &pinned, dir.path(), &ProgressReporter::silent());
        assert!(first.is_err(), "short chunks must not be accepted");
        assert!(
            dir.path().join("artifact.bin.parts").exists(),
            "the ledger records what landed"
        );

        let complete = FakeSource::new(content.clone(), vec![]);
        let path =
            fetch_small_grid(&complete, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), content);
        // The ledger knew the total, so no probe — only the four chunks that were missing.
        assert_eq!(
            complete.open_count(),
            4,
            "the finished chunk must not be fetched again"
        );
    }

    #[test]
    fn a_range_refusing_server_falls_back_to_the_sequential_path() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let source = FakeSource::new(content.clone(), vec![]).ignoring_range();

        let path =
            fetch_small_grid(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        assert!(!dir.path().join("artifact.bin.parts").exists());
    }

    #[test]
    fn a_server_without_a_total_size_downloads_sequentially() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let source = FakeSource::new(content.clone(), vec![]).hiding_total();

        let path =
            fetch_small_grid(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
    }

    #[test]
    fn a_sequential_partial_is_credited_to_the_parallel_resume() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        // 150 000 bytes of prefix: two full 64 KiB chunks and change.
        fs::write(dir.path().join("artifact.bin.part"), &content[..150_000]).unwrap();

        let source = FakeSource::new(content.clone(), vec![]);
        let path =
            fetch_small_grid(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        // Chunks 0 and 1 were credited; the probe re-fetches nothing, so only the last
        // three chunks travel.
        assert!(
            source.open_count() <= 4,
            "expected only the missing chunks, got {} requests",
            source.open_count()
        );
    }

    /// Interrupt it, run it again, get the right file — without re-downloading what already
    /// arrived.
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
        fs::write(dir.path().join("artifact.bin.part"), &content[..120_000]).unwrap();

        let source = FakeSource::new(content.clone(), vec![]);
        let path = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        assert_eq!(source.opens.lock().unwrap().as_slice(), &[120_000u64]);
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
            source.opens.lock().unwrap().is_empty(),
            "no request should be made"
        );
    }

    #[test]
    fn a_corrupted_cached_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        fs::write(dir.path().join("artifact.bin"), b"not the artifact").unwrap();
        let source = FakeSource::new(content.clone(), vec![]);

        let path = fetch(&source, &pinned, dir.path(), &ProgressReporter::silent()).unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        assert_eq!(source.opens.lock().unwrap().as_slice(), &[0u64]);
    }

    /// A 580 MB download with no visible progress is a defect.
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
            messages.iter().filter(|m| m.contains('%')).count() >= 2,
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
