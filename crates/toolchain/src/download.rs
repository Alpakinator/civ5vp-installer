//! Fetching a pinned artifact: resume what is half-there, prove it against its SHA-256, and
//! only then let it into the Toolchain Cache.
//!
//! The shape is deliberate. A big download over a slow connection will be interrupted, so
//! bytes land in `<name>.part` and the finished, *verified* file appears at `<name>` in one
//! atomic rename. Anything that goes wrong leaves either a resumable `.part` or nothing -
//! never a short file that looks finished.
//!
//! Large artifacts download over **several connections at once**. The Wayback Machine - the
//! one source of the pinned SDK image - throttles per connection to roughly 1 MB/s, and
//! measured from a real machine four parallel ranged requests deliver about 4.5x the
//! single-connection rate. The file is divided into a fixed grid of chunks, each fetched
//! with its own ranged request; a sidecar (`<name>.parts`) records which chunks are done, so
//! an interrupted run redoes only what is missing. A server that ignores `Range` drops the
//! whole fetch back to the sequential path automatically.
//!
//! **Not every download is a whole file.** The Windows SDK image is 1.45 GiB of which the
//! build reads about 102 MiB, so its members are fetched as *windows*: the same URL, a
//! `Range` over one member's own bytes, checked against that member's own SHA-256
//! (`docs/pinned-artifacts.md` §1). Everything below - the chunk grid, the ledger, the
//! resume, the retries - works in coordinates relative to what was asked for, so a window
//! behaves exactly like a small artifact that happens to live inside a large one.
//!
//! **A failed request is the normal case, not the exceptional one.** The Wayback Machine
//! answers a ranged request with nothing at all often enough that `docs/pinned-artifacts.md`
//! §1 records it, so one dropped connection must never end a 1.45 GiB download: every ranged
//! request is retried with a widening pause, the whole download is picked up again a few
//! times after that, and every request carries a deadline so a wedged socket fails instead of
//! holding its worker - and with it a quarter of the throughput - forever.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use civ5vp_core::{ProgressReporter, Stage};
use sha2::{Digest, Sha256};

use crate::error::{ToolchainError, io_error};
use crate::pinned::{PinnedDownload, PinnedMember};

/// Copy buffer. Big enough that the syscall overhead disappears against a network stream.
const CHUNK: usize = 256 * 1024;

/// How much has to arrive before progress speaks again. At ~1 MB/s that is a line every
/// eight seconds; at 100 MB/s it is not a flood.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// One ranged request's worth of file in the parallel path.
///
/// A dropped connection costs whatever the chunk had not finished, so this is a trade
/// between the waste of a failure (8 MB, seconds) and the ~10 s the Wayback Machine takes to
/// answer at all - at 8 MB a chunk that overhead is a few percent of the transfer.
const PARALLEL_CHUNK: u64 = 8 * 1024 * 1024;

/// How many connections fetch chunks at once. Four is where the Wayback Machine's
/// per-connection throttle stops being the limit, and modest enough to stay polite.
const PARALLEL_CONNECTIONS: usize = 4;

/// Artifacts smaller than this are not worth the extra requests.
const PARALLEL_THRESHOLD: u64 = 64 * 1024 * 1024;

/// How many times one ranged request is tried before its chunk is given up on.
const CHUNK_ATTEMPTS: u32 = 5;

/// How many times the whole download is picked up again after a failure. Each pass resumes
/// from the ledger, so a pass costs only what had not arrived yet.
const DOWNLOAD_PASSES: u32 = 3;

/// The pause after the first failure. Each further one doubles, up to [`BACKOFF_CAP`] -
/// enough to ride out a server that is briefly refusing, short enough not to look hung.
const BACKOFF: Duration = Duration::from_secs(2);
const BACKOFF_CAP: Duration = Duration::from_secs(30);

/// How long a connection may take to open, and how long the server may think before it
/// answers with headers. Measured time to first byte on the pinned image is 9-14 s
/// (`docs/pinned-artifacts.md` §1), so these are generous rather than tight.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(90);

/// The slowest a ranged body may crawl before it counts as wedged. `ureq`'s body timeout is
/// for the whole body, so a chunk's deadline is derived from its length: without one, a
/// stalled socket blocks its worker forever and four of those are the entire download.
const SLOWEST_TOLERATED: u64 = 16 * 1024;

/// Floor under that derivation, so a short range still gets a sane minimum.
const MIN_BODY_TIMEOUT: Duration = Duration::from_secs(60);

/// The sequential path streams a whole artifact in one body, whose length is not known when
/// the request is built, so its deadline cannot be derived the same way. This is a backstop
/// against a socket that never closes, not a rate: the slowest measured source still fits.
const SEQUENTIAL_BODY_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

/// A stream of bytes starting partway into a resource - the one thing the downloader needs
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
        // Every `ureq` timeout defaults to "none at all", which for a 1.45 GiB artifact off a
        // replay service means a single wedged socket can hold a worker until the user gives
        // up. The pool is widened to one idle connection per worker as well, so ~180 chunk
        // requests reuse four sockets rather than paying a TLS handshake each.
        Self {
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_connect(Some(CONNECT_TIMEOUT))
                    .timeout_recv_response(Some(RESPONSE_TIMEOUT))
                    .max_idle_connections_per_host(PARALLEL_CONNECTIONS)
                    .build(),
            ),
        }
    }

    fn request(
        &self,
        url: &str,
        range: Option<String>,
        body_timeout: Duration,
    ) -> Result<Transfer, ToolchainError> {
        let mut request = self
            .agent
            .get(url)
            .config()
            .timeout_recv_body(Some(body_timeout))
            .build();
        if let Some(range) = &range {
            request = request.header("Range", range.as_str());
        }
        let response = request.call().map_err(|error| network_error(url, &error))?;

        let status = response.status().as_u16();
        // 206 means the Range header was honoured. Anything else 2xx is the whole resource,
        // which is still usable - it just means starting over (or, on the parallel path,
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

/// How long a ranged body of `bytes` may take before it counts as wedged rather than slow.
fn body_timeout_for(bytes: u64) -> Duration {
    Duration::from_secs(bytes / SLOWEST_TOLERATED).max(MIN_BODY_TIMEOUT)
}

impl ByteSource for HttpByteSource {
    fn open(&self, url: &str, offset: u64) -> Result<Transfer, ToolchainError> {
        let range = (offset > 0).then(|| format!("bytes={offset}-"));
        let mut transfer = self.request(url, range, SEQUENTIAL_BODY_TIMEOUT)?;
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
        let mut transfer = self.request(
            url,
            Some(format!("bytes={start}-{}", end - 1)),
            body_timeout_for(end.saturating_sub(start)),
        )?;
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
        Wanted::whole(pinned),
        downloads_dir,
        progress,
        Tuning::shipped(),
    )
}

/// Make one member of `image` present and verified in `downloads_dir`, without downloading
/// the rest of the image.
///
/// The four pinned members are ~102 MiB of a 1.45 GiB disc image, and the only place that
/// image still exists is a replay service that serves it at a fraction of a megabyte a
/// second - so fetching the other 93% of it is most of a first bootstrap's wall-clock time,
/// spent on bytes that are read by nothing. Each member is one run of bytes at a pinned offset
/// (`docs/pinned-artifacts.md` §1), so it is one windowed download like any other, checked
/// against its own SHA-256.
pub fn fetch_member(
    source: &dyn ByteSource,
    image: &PinnedDownload,
    member: &PinnedMember,
    label: &str,
    downloads_dir: &Path,
    progress: &ProgressReporter,
) -> Result<PathBuf, ToolchainError> {
    let cache_name = member.cache_name();
    fetch_with(
        source,
        Wanted {
            file_name: &cache_name,
            label,
            url: image.url,
            sha256: member.sha256,
            offset: member.offset,
            bytes: Some(member.bytes),
            approximate_bytes: member.bytes,
        },
        downloads_dir,
        progress,
        Tuning::for_member(),
    )
}

/// One thing to fetch: what it is called on disk, where its bytes are, and what they have to
/// hash to.
///
/// The window is what makes a member of the disc image expressible: same URL, but only the
/// bytes that member occupies. A whole artifact is the degenerate case - offset 0, and a
/// length only the server can say.
#[derive(Debug, Clone, Copy)]
struct Wanted<'a> {
    file_name: &'a str,
    /// What to call this in a sentence a player reads. A file name will do for an artifact
    /// that *is* a file; a member of a disc image is "the Windows SDK core", not
    /// `WinSDK-WinSDK_x86.msi`.
    label: &'a str,
    url: &'a str,
    sha256: &'a str,
    /// Where the wanted bytes start in the resource.
    offset: u64,
    /// How many bytes are wanted, when that is known before asking.
    bytes: Option<u64>,
    /// Roughly how many bytes that is, for the decisions and the sentences that have to be
    /// made before the server answers.
    approximate_bytes: u64,
}

impl<'a> Wanted<'a> {
    fn whole(pinned: &'a PinnedDownload) -> Self {
        Self {
            file_name: pinned.file_name,
            label: pinned.file_name,
            url: pinned.url,
            sha256: pinned.sha256,
            offset: 0,
            bytes: None,
            approximate_bytes: pinned.approximate_bytes,
        }
    }

    /// Whether this is a slice of a larger resource. A server that will not serve ranges can
    /// still deliver a whole artifact; it cannot deliver a window at all.
    fn is_window(&self) -> bool {
        self.bytes.is_some()
    }
}

/// The numbers behind a download: the parallel grid, and how stubbornly a failed request is
/// tried again.
///
/// One struct rather than seven arguments, because the fast suite turns all of them down at
/// once - kilobyte chunks, and no waiting between tries, since a test that sleeps for the
/// real backoff is a test nobody runs.
#[derive(Debug, Clone, Copy)]
struct Tuning {
    chunk_size: u64,
    parallel_threshold: u64,
    connections: usize,
    /// Tries per ranged request, including the first.
    attempts: u32,
    /// Tries at the whole download, including the first.
    passes: u32,
    /// The pause after the first failure; each further one doubles.
    backoff: Duration,
}

impl Tuning {
    /// What the installer actually downloads with.
    fn shipped() -> Self {
        Self {
            chunk_size: PARALLEL_CHUNK,
            parallel_threshold: PARALLEL_THRESHOLD,
            connections: PARALLEL_CONNECTIONS,
            attempts: CHUNK_ATTEMPTS,
            passes: DOWNLOAD_PASSES,
            backoff: BACKOFF,
        }
    }

    /// The same, with a lower bar for going parallel.
    ///
    /// The threshold exists so a small artifact is not split into requests that cost more
    /// than they save - but that reasoning is about the *probe*, and a member's length is
    /// pinned, so there is nothing to discover. The two large cabinets (30 MB and 43 MB) are
    /// most of what a first bootstrap fetches and are well under [`PARALLEL_THRESHOLD`].
    fn for_member() -> Self {
        Self {
            parallel_threshold: 2 * PARALLEL_CHUNK,
            ..Self::shipped()
        }
    }
}

/// [`fetch`] with the grid and the retries as parameters, so the fast suite can exercise the
/// chunking with kilobytes instead of gigabytes.
fn fetch_with(
    source: &dyn ByteSource,
    wanted: Wanted<'_>,
    downloads_dir: &Path,
    progress: &ProgressReporter,
    tuning: Tuning,
) -> Result<PathBuf, ToolchainError> {
    fs::create_dir_all(downloads_dir)
        .map_err(|error| io_error("create the downloads folder", downloads_dir, &error))?;

    let final_path = downloads_dir.join(wanted.file_name);
    let partial_path = downloads_dir.join(format!("{}.part", wanted.file_name));
    let sidecar_path = downloads_dir.join(format!("{}.parts", wanted.file_name));

    if final_path.is_file() {
        // Present from an earlier run. Still hashed: a file that was truncated by a full disk
        // or edited by something else must not be handed to the extractor.
        if hash_file(&final_path)? == wanted.sha256 {
            progress.report(
                Stage::Build,
                format!("Already have {} - skipping the download.", wanted.label),
            );
            let _ = fs::remove_file(&sidecar_path);
            return Ok(final_path);
        }
        fs::remove_file(&final_path)
            .map_err(|error| io_error("remove a damaged download", &final_path, &error))?;
    }

    // A pass that stops early leaves its bytes in the `.part` and its progress in the
    // ledger, so picking the download up again asks only for what is still missing. This is
    // what keeps one dropped connection from costing the user a click and an hour.
    with_retries(
        tuning.passes,
        tuning.backoff,
        progress,
        |next| {
            format!(
                "The download of {} stopped early - carrying on from where it stopped (attempt {next} of {}).",
                wanted.label, tuning.passes
            )
        },
        || {
            let mut fetched_in_parallel = false;
            if wanted.approximate_bytes >= tuning.parallel_threshold {
                fetched_in_parallel = parallel_download(
                    source,
                    wanted,
                    &partial_path,
                    &sidecar_path,
                    progress,
                    tuning,
                )?;
            }
            if !fetched_in_parallel {
                download_to_partial(source, wanted, &partial_path, progress)?;
            }
            Ok(())
        },
    )?;

    let actual = hash_file(&partial_path)?;
    if actual != wanted.sha256 {
        // Do not keep it: resuming from bytes that are already wrong would loop forever.
        let _ = fs::remove_file(&partial_path);
        let _ = fs::remove_file(&sidecar_path);
        return Err(ToolchainError::new(
            format!(
                "The download of {} came out damaged. Check your connection and try again.",
                wanted.label
            ),
            format!(
                "sha256 mismatch for {} at offset {}: expected {}, got {actual}",
                wanted.url, wanted.offset, wanted.sha256
            ),
        ));
    }

    // The rename is the commit point: `<name>` exists only once its contents are proven.
    fs::rename(&partial_path, &final_path)
        .map_err(|error| io_error("finish a download", &final_path, &error))?;
    let _ = fs::remove_file(&sidecar_path);
    progress.report(
        Stage::Build,
        format!("Downloaded {} and checked it.", wanted.label),
    );
    Ok(final_path)
}

/// Run `attempt` until it succeeds or the tries run out, pausing longer after each failure.
///
/// `announce` phrases the wait for the activity log, given the number of the try about to
/// start: a download that is quietly waiting thirty seconds and a download that has died
/// look identical from the outside otherwise.
fn with_retries<T>(
    attempts: u32,
    backoff: Duration,
    progress: &ProgressReporter,
    announce: impl Fn(u32) -> String,
    mut attempt: impl FnMut() -> Result<T, ToolchainError>,
) -> Result<T, ToolchainError> {
    let mut pause = backoff;
    for try_number in 1..attempts.max(1) {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(_) => {
                progress.report(Stage::Build, announce(try_number + 1));
                std::thread::sleep(pause);
                pause = (pause * 2).min(BACKOFF_CAP);
            }
        }
    }
    // The last try's error is the one the user sees, so it is not swallowed by a retry.
    attempt()
}

/// Take a lock back after a worker panicked.
///
/// The bytes that worker wrote are on disk either way, so treating the poison as a reason to
/// drop the ledger would lose them - which is the opposite of what the ledger is for.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

/// Download `wanted` over several ranged connections.
///
/// Returns `Ok(false)` - with nothing torn down - when the server turns out not to support
/// ranges or not to say the total size; the caller then uses the sequential path. `Ok(true)`
/// means the `.part` file holds every byte.
fn parallel_download(
    source: &dyn ByteSource,
    wanted: Wanted<'_>,
    partial_path: &Path,
    sidecar_path: &Path,
    progress: &ProgressReporter,
    tuning: Tuning,
) -> Result<bool, ToolchainError> {
    let chunk_size = tuning.chunk_size;
    let connections = tuning.connections;
    // A ledger from an interrupted parallel run already knows the total, so a resume asks
    // for nothing but the missing chunks. A window knows it too, from the pin. Otherwise the
    // first chunk doubles as the probe: it learns the exact total and whether the server
    // honours ranges, and its bytes are never wasted.
    let mut probe = None;
    let mut ledger = match (ChunkLedger::load(sidecar_path, chunk_size), wanted.bytes) {
        (Some(ledger), _) => ledger,
        (None, Some(bytes)) => ChunkLedger::new(chunk_size, bytes),
        (None, None) => {
            let opened = source.open_range(wanted.url, 0, chunk_size)?;
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
            "Downloading {} ({}){}.",
            wanted.label,
            human_bytes(total),
            if already > 0 {
                format!(" - resuming with {} already here", human_bytes(already))
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
        let written = write_chunk(partial_path, probe, start, end, wanted.url);
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

    // Workers pull chunk indices from a shared cursor. A chunk that fails is asked for again
    // - the Wayback Machine drops ranged requests routinely, and one dropped request is not
    // a reason to abandon a gigabyte. Only a chunk that fails every try, or a server that
    // turns out not to serve ranges at all, stops the others.
    let cursor = AtomicUsize::new(0);
    let range_refused = AtomicBool::new(false);
    let failed: Mutex<Option<ToolchainError>> = Mutex::new(None);
    let finished: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let downloaded = AtomicU64::new(already);
    let announced = AtomicU64::new(already);
    let started = Instant::now();
    let chunk_count = ledger.done.len();

    std::thread::scope(|scope| {
        for _ in 0..connections.max(1) {
            scope.spawn(|| {
                loop {
                    if range_refused.load(Ordering::Relaxed) || locked(&failed).is_some() {
                        return;
                    }
                    let slot = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(&index) = todo.get(slot) else {
                        return;
                    };
                    let (start, end) = ledger.range_of(index);
                    let outcome = with_retries(
                        tuning.attempts,
                        tuning.backoff,
                        progress,
                        |next| {
                            format!(
                                "Part {} of {chunk_count} of {} did not arrive - asking again \
                                 (attempt {next} of {}).",
                                index + 1,
                                wanted.label,
                                tuning.attempts
                            )
                        },
                        || {
                            // The grid is over the wanted bytes; the requests are over the
                            // resource, which for a member of the disc image starts a
                            // gigabyte into it.
                            let from = wanted.offset + start;
                            let to = wanted.offset + end;
                            let transfer = source.open_range(wanted.url, from, to)?;
                            // Not a failure to retry: this server does not do ranges, and
                            // asking it four more times will not change that.
                            if transfer.start != from {
                                return Ok(false);
                            }
                            write_chunk(partial_path, transfer, start, end, wanted.url)?;
                            Ok(true)
                        },
                    );
                    match outcome {
                        Ok(false) => {
                            range_refused.store(true, Ordering::Relaxed);
                            return;
                        }
                        Ok(true) => {
                            locked(&finished).push(index);
                            let bytes = end - start;
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
                                        "Downloading {} - {} of {} ({}%) at {}.",
                                        wanted.label,
                                        human_bytes(so_far),
                                        human_bytes(total),
                                        percent(so_far, total),
                                        human_rate(so_far - already, started.elapsed())
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            locked(&failed).get_or_insert(error);
                            return;
                        }
                    }
                }
            });
        }
    });

    // Record everything that landed, whatever else happened - that is the resume state.
    for &index in locked(&finished).iter() {
        ledger.done[index] = true;
    }
    ledger.save(sidecar_path)?;

    if let Some(error) = locked(&failed).take() {
        return Err(error);
    }
    if range_refused.load(Ordering::Relaxed) {
        if wanted.is_window() {
            // A whole artifact can still be had from a server that ignores `Range`; one
            // member out of the middle of a disc image cannot be had at all.
            return Err(ToolchainError::new(
                "The download server stopped letting the installer ask for parts of files, \
                 so the Windows SDK cannot be fetched piece by piece. Try again later.",
                format!(
                    "{} ignored a Range request for {}+{} bytes",
                    wanted.url,
                    wanted.offset,
                    wanted.bytes.unwrap_or_default()
                ),
            ));
        }
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

    let missing = ledger.done.iter().filter(|done| !**done).count();
    if missing > 0 {
        // Every other way out of the loop is accounted for above, so this means a worker
        // died without recording why. Saying so is the point: the alternative is returning
        // "not fetched in parallel" and letting the sequential path quietly restart a
        // gigabyte, which is how a failed download comes to look like a finished one.
        return Err(ToolchainError::new(
            "The download stopped before it finished. Try again - the installer will carry \
             on from where it stopped.",
            format!("{missing} chunks of {} are still missing", wanted.file_name),
        ));
    }
    Ok(true)
}

/// Stream one transfer into `[start, end)` of the partial file. Short bodies are an error -
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
                "The download was interrupted. Try again - the installer will carry on from \
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
// The sequential path - for small artifacts and servers without ranges
// ---------------------------------------------------------------------------------------

fn download_to_partial(
    source: &dyn ByteSource,
    wanted: Wanted<'_>,
    partial_path: &Path,
    progress: &ProgressReporter,
) -> Result<(), ToolchainError> {
    let already = match fs::metadata(partial_path) {
        Ok(metadata) => metadata.len(),
        Err(_) => 0,
    };

    // A window asks for its own bytes and nothing else; a whole artifact asks from where it
    // stopped and lets the server say how much there is.
    let mut transfer = match wanted.bytes {
        Some(bytes) => {
            source.open_range(wanted.url, wanted.offset + already, wanted.offset + bytes)?
        }
        None => source.open(wanted.url, already)?,
    };
    if wanted.is_window() && transfer.start != wanted.offset + already {
        // The server ignored the range and started somewhere else. For a whole artifact that
        // is merely a slower path; for a window it is the wrong bytes, and they would be
        // written into a file named after the member they are not.
        return Err(ToolchainError::new(
            "The download server stopped letting the installer ask for parts of files, so \
             the Windows SDK cannot be fetched piece by piece. Try again later.",
            format!(
                "{} answered a request for {}+{} with bytes starting at {}",
                wanted.url,
                wanted.offset + already,
                wanted.bytes.unwrap_or_default(),
                transfer.start
            ),
        ));
    }
    // From here on, positions are relative to what is wanted rather than to the resource it
    // lives in: the `.part` holds the member, not the image the member came out of.
    transfer.start -= wanted.offset;
    // What the server said the whole resource is, as opposed to what the pin guesses. Only
    // one of them can say whether the body that arrives is the whole body.
    let announced_total = wanted.bytes.or(transfer.total);
    let expected_total = announced_total.unwrap_or(wanted.approximate_bytes);

    let mut file = if transfer.start > 0 {
        progress.report(
            Stage::Build,
            format!(
                "Resuming the download of {} at {}.",
                wanted.label,
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
                wanted.label,
                human_bytes(expected_total)
            ),
        );
        File::create(partial_path)
            .map_err(|error| io_error("create a partial download", partial_path, &error))?
    };

    let mut written = transfer.start;
    let mut announced = written;
    let resumed_at = written;
    let started = Instant::now();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = transfer
            .body
            .read(&mut buffer)
            .map_err(|error| network_read_error(wanted.url, &error))?;
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
                    "Downloading {} - {} of {} ({}%) at {}.",
                    wanted.label,
                    human_bytes(written),
                    human_bytes(expected_total),
                    percent(written, expected_total),
                    human_rate(written - resumed_at, started.elapsed())
                ),
            );
        }
    }
    file.flush()
        .map_err(|error| io_error("flush a partial download", partial_path, &error))?;

    // A body that ends early is a dropped connection, not a finished download. Saying so
    // here is what makes the next pass resume from these bytes; letting it through would
    // hand a short file to the hash, which deletes every byte of it.
    if let Some(total) = announced_total
        && written < total
    {
        return Err(ToolchainError::new(
            "The download was interrupted. Try again - the installer will carry on from \
             where it stopped.",
            format!(
                "{} ended {} bytes short of {total}",
                wanted.file_name,
                total - written
            ),
        ));
    }
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

/// How fast bytes are arriving, phrased for a player. Under a megabyte a second it is
/// kilobytes, because "0.2 MB/s" reads like a rounding error rather than a speed - and on
/// the pinned SDK image 0.2 MB/s is the honest figure.
fn human_rate(bytes: u64, over: Duration) -> String {
    let seconds = over.as_secs_f64();
    if seconds <= 0.0 {
        return "0 KB/s".to_owned();
    }
    let rate = bytes as f64 / seconds;
    const MB: f64 = 1024.0 * 1024.0;
    if rate >= MB {
        format!("{:.1} MB/s", rate / MB)
    } else {
        format!("{:.0} KB/s", rate / 1024.0)
    }
}

fn human_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (MB * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MB", bytes as f64 / MB)
    } else {
        // One pinned member is a 400 KB MSI, and "0 MB" is not a size.
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn network_error(url: &str, error: &ureq::Error) -> ToolchainError {
    ToolchainError::new(
        "The installer could not reach the download server. Check your internet connection \
         and try again - anything already downloaded is kept.",
        format!("request to {url} failed: {error}"),
    )
}

fn network_read_error(url: &str, error: &std::io::Error) -> ToolchainError {
    ToolchainError::new(
        "The download was interrupted. Try again - the installer will carry on from where it \
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

    /// A ByteSource over an in-memory blob, which can be told to stop short - the fast
    /// suite's stand-in for a dropped connection. `Mutex` inside because the parallel path
    /// shares the source across worker threads.
    struct FakeSource {
        content: Vec<u8>,
        /// How many bytes each successive `open` will hand over before ending the stream.
        allowances: Mutex<Vec<usize>>,
        /// Once the allowances run out, how much every further `open` hands over. `None` is
        /// a server that has recovered; `Some` is one that never manages a whole range.
        always_stops_after: Option<usize>,
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
                always_stops_after: None,
                ignores_range: false,
                hides_total: false,
                opens: Mutex::new(Vec::new()),
            }
        }

        fn always_stopping_after(mut self, bytes: usize) -> Self {
            self.always_stops_after = Some(bytes);
            self
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
            match (
                self.allowances.lock().unwrap().pop(),
                self.always_stops_after,
            ) {
                (Some(allowance), _) => remaining.truncate(allowance),
                (None, Some(limit)) => remaining.truncate(limit),
                (None, None) => {}
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

    /// A small parallel grid - 64 KiB chunks, everything over 100 KiB parallel on three
    /// connections - and, unless a test says otherwise, one try at everything: most of these
    /// tests are about what happens on a failure, which retries would paper over.
    fn small_grid() -> Tuning {
        Tuning {
            chunk_size: 64 * 1024,
            parallel_threshold: 100 * 1024,
            connections: 3,
            attempts: 1,
            passes: 1,
            // Zero, always: a test that waits out the shipped backoff is a test nobody runs.
            backoff: Duration::ZERO,
        }
    }

    /// The sequential grid: nothing is large enough to go parallel.
    fn sequential() -> Tuning {
        Tuning {
            parallel_threshold: u64::MAX,
            ..small_grid()
        }
    }

    fn fetch_small_grid(
        source: &dyn ByteSource,
        pinned: &PinnedDownload,
        dir: &Path,
        progress: &ProgressReporter,
    ) -> Result<PathBuf, ToolchainError> {
        fetch_tuned(source, pinned, dir, progress, small_grid())
    }

    /// [`fetch`] at a stated tuning, on a whole artifact.
    fn fetch_tuned(
        source: &dyn ByteSource,
        pinned: &PinnedDownload,
        dir: &Path,
        progress: &ProgressReporter,
        tuning: Tuning,
    ) -> Result<PathBuf, ToolchainError> {
        fetch_with(source, Wanted::whole(pinned), dir, progress, tuning)
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
        // The ledger knew the total, so no probe - only the four chunks that were missing.
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

    /// A body that stops early is a dropped connection. It must not reach the hash, which
    /// would delete every byte that did arrive.
    #[test]
    fn a_body_that_ends_early_is_not_treated_as_a_finished_download() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let truncating = FakeSource::new(content.clone(), vec![100_000]);

        let error = fetch_tuned(
            &truncating,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
            sequential(),
        )
        .unwrap_err();

        assert!(
            error.detail().contains("short of"),
            "expected a short-body detail, got {}",
            error.detail()
        );
        assert_eq!(
            fs::metadata(dir.path().join("artifact.bin.part"))
                .unwrap()
                .len(),
            100_000,
            "the bytes that arrived are kept for the next attempt"
        );
    }

    /// The connection drops once. The user should not have to notice, let alone click
    /// Install again.
    #[test]
    fn a_dropped_connection_is_picked_up_by_the_next_pass() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let truncating = FakeSource::new(content.clone(), vec![100_000]);

        let path = fetch_tuned(
            &truncating,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
            Tuning {
                passes: 2,
                ..sequential()
            },
        )
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        // The second pass asked for the rest, not for the whole thing again.
        assert_eq!(
            truncating.opens.lock().unwrap().as_slice(),
            &[0u64, 100_000]
        );
    }

    /// Interrupt it, run it again, get the right file - without re-downloading what already
    /// arrived.
    #[test]
    fn an_interrupted_download_resumes_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);

        let truncating = FakeSource::new(content.clone(), vec![100_000]);
        let first = fetch_tuned(
            &truncating,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
            sequential(),
        );
        assert!(first.is_err(), "a short download must not be accepted");
        assert!(!dir.path().join("artifact.bin").exists());

        // A second run, as if the user clicked Install again: it continues from the 100 000
        // bytes the first one left behind rather than starting over.
        let complete = FakeSource::new(content.clone(), vec![]);
        let path = fetch_tuned(
            &complete,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
            sequential(),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), content);
        assert_eq!(complete.opens.lock().unwrap().as_slice(), &[100_000u64]);
    }

    /// The Wayback Machine answers a ranged request with nothing at all often enough that
    /// `docs/pinned-artifacts.md` §1 records it. One such answer must cost a chunk, not a
    /// gigabyte.
    #[test]
    fn a_dropped_chunk_is_asked_for_again_rather_than_failing_the_download() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        // Allowances pop from the end: the probe gets its whole chunk, the next request cut
        // short, everything after that is served properly.
        let flaky = FakeSource::new(content.clone(), vec![1000, 65536]);

        let path = fetch_tuned(
            &flaky,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
            Tuning {
                attempts: 3,
                ..small_grid()
            },
        )
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), content);
        // 300 KB over 64 KiB chunks is the probe plus four chunks, plus the one retry.
        assert_eq!(flaky.open_count(), 6);
        assert!(!dir.path().join("artifact.bin.parts").exists());
    }

    /// A server that never manages a whole range is a real failure - but the bytes that did
    /// arrive stay on disk, and the sentence tells the user what to do about it.
    #[test]
    fn a_chunk_that_never_arrives_ends_the_download_with_a_sentence() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let broken = FakeSource::new(content.clone(), vec![65536]).always_stopping_after(1000);

        let error = fetch_tuned(
            &broken,
            &pinned,
            dir.path(),
            &ProgressReporter::silent(),
            Tuning {
                attempts: 2,
                ..small_grid()
            },
        )
        .unwrap_err();

        assert!(
            error.message().contains("carry on from where it"),
            "expected a resumable-sounding message, got {}",
            error.message()
        );
        assert!(
            dir.path().join("artifact.bin.parts").exists(),
            "the ledger records the probe's chunk for the next attempt"
        );
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
        // A rate is what tells a player that 0.2 MB/s is the artifact's fault and not a
        // frozen installer.
        assert!(
            messages.iter().any(|m| m.contains("/s")),
            "expected a transfer rate, got {messages:?}"
        );
    }

    /// Waiting quietly and having died look the same from the outside, so the wait is said
    /// out loud.
    #[test]
    fn a_retry_says_so_rather_than_pausing_in_silence() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let pinned = pinned_for(&content);
        let flaky = FakeSource::new(content, vec![1000, 65536]);

        let (sender, receiver) = std::sync::mpsc::channel();
        fetch_tuned(
            &flaky,
            &pinned,
            dir.path(),
            &ProgressReporter::to_channel(sender),
            Tuning {
                attempts: 3,
                ..small_grid()
            },
        )
        .unwrap();

        let messages: Vec<String> = receiver.iter().map(|event| event.message).collect();
        assert!(
            messages.iter().any(|m| m.contains("asking again")),
            "expected the retry to be announced, got {messages:?}"
        );
    }

    /// A member of the disc image is the same URL and a window into it - and nothing outside
    /// that window is ever asked for, which is the entire point.
    #[test]
    fn a_member_is_fetched_out_of_the_middle_of_the_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let image = pinned_for(&content);
        let window = &content[100_000..180_000];
        let digest = hex(&Sha256::digest(window));
        let member = PinnedMember {
            path: "Setup/WinSDK/cab1.cab",
            offset: 100_000,
            bytes: 80_000,
            sha256: Box::leak(digest.into_boxed_str()),
        };
        let source = FakeSource::new(content.clone(), vec![]);

        let path = fetch_member(
            &source,
            &image,
            &member,
            "the Windows SDK core",
            dir.path(),
            &ProgressReporter::silent(),
        )
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), window);
        assert_eq!(path.file_name().unwrap(), "WinSDK-cab1.cab");
        assert_eq!(
            source.opens.lock().unwrap().as_slice(),
            &[100_000u64],
            "only the member's own bytes may be requested"
        );
    }

    /// A server that ignores `Range` can still deliver a whole artifact. It cannot deliver
    /// one member out of the middle of one, and pretending otherwise would write the front of
    /// the image into a file named after a cabinet.
    #[test]
    fn a_member_cannot_be_fetched_from_a_server_that_ignores_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let content = content();
        let image = pinned_for(&content);
        let member = PinnedMember {
            path: "Setup/WinSDK/cab1.cab",
            offset: 100_000,
            bytes: 80_000,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        };
        let source = FakeSource::new(content, vec![]).ignoring_range();

        let error = fetch_member(
            &source,
            &image,
            &member,
            "the Windows SDK core",
            dir.path(),
            &ProgressReporter::silent(),
        )
        .unwrap_err();

        assert!(
            error.message().contains("parts of files"),
            "expected a sentence about ranges, got {}",
            error.message()
        );
    }

    #[test]
    fn byte_sizes_read_the_way_a_player_expects() {
        assert_eq!(human_bytes(580 * 1024 * 1024), "580 MB");
        assert_eq!(human_bytes(408_576), "399 KB");
        assert_eq!(
            human_bytes(1024 * 1024 * 1024 + 512 * 1024 * 1024),
            "1.5 GB"
        );
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(1, 0), 0);
    }
}
