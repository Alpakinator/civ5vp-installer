# Download-speed analysis — fresh-user run, 2026-08-14

Measured live during a real fresh install (empty App Data Store, all artifacts
re-downloaded), sampled every 10 s by a monitor script logging allocated bytes.
Two installer binaries took part: the run began on the sequential downloader and
was restarted mid-ISO onto the parallel one (commit 0bcdc15), which credited the
sequential prefix and continued — an accidental but perfect A/B on the same
machine, file, and server.

## Measured rates by source

| Artifact | Server | Method | Observed rate |
|---|---|---|---|
| GRMSDK_EN_DVD.iso (1,480 MB) | web.archive.org | sequential, 1 conn | ~1.0–1.3 MB/s |
| GRMSDK_EN_DVD.iso | web.archive.org | parallel, 4 × 32 MiB ranges | 2–6 MB/s sustained, 10–20 MB/s peaks |
| clang+llvm 18.1.8 (996 MB) | GitHub releases | sequential | **10.6 MB/s** (91 s total) |
| VP sources (~490 MB) | GitHub (gix clone) | — | fast, never the bottleneck |
| SDK extraction (559 MB out) | local CPU | — | ~27 MB/s written, well under a minute |

Archive.org is the whole problem. Its per-connection throughput varies wildly by
backend node (0.4 → 5 MB/s per stream at different moments); parallel ranges
multiply whatever each stream gets, which is why the observed spread is wide.
GitHub saturates a home connection with a single stream.

## Incident: connection refused at 89 %

After ~1.3 GB of sustained multi-connection transfer, archive.org refused new
connections; one failed range request aborted the entire install ("The installer
could not reach the download server"). The chunk ledger made the retry cheap
(only the missing 6 of 47 chunks were fetched — resume verified working), but a
fresh user would have seen a scary failure at 89 % through the biggest download.
A curl probe ~2 min later got HTTP 206 again: the refusal was transient
rate-limiting, not an outage.

## Improvements, ranked by user impact

1. **Retry transient network errors with backoff** (a few attempts, seconds
   apart) inside the download workers instead of failing the install. This
   converts the observed 89 %-death into a pause nobody notices. Cheap; do first.
2. **Ranged extraction of only the needed ISO members.** The build uses ~200 MB
   of the 1,480 MB ISO (headers, libs, the VC9 CRT cabinets). The UDF directory
   can be parsed from a few small ranged reads, then only the needed extents
   fetched. ~7× less data from the slowest server: worst-case archive.org
   (1 MB/s) drops from ~25 min to ~3 min. Largest single win; needs UDF
   extent-mapping work in the extractor.
3. **Keep parallelism at 4 connections.** The refusal incident suggests
   archive.org pushes back on heavy clients; more connections would raise both
   risk and little benefit once ranged extraction lands.
4. LLVM/GitHub needs nothing — already the fast path. Bundling LLVM in the
   installer was considered and rejected (adds ~1 GB to a ~10 MB binary);
   bundling the SDK is not legally redistributable at all.

## Fresh-install wall clock (this machine, ~50 Mbit effective to archive.org)

With the parallel downloader from the start and no failure, the projected
fresh-install total is ~12–15 min, dominated by the ISO (~6–12 min). With
improvement #2 the ISO leg shrinks to ~1–3 min and the total approaches ~7 min:
sources ≈ 2 min, LLVM ≈ 1.5 min, extraction ≈ 2 min, compile+link ≈ 1.5 min.

Raw samples: download-speeds.csv (scratchpad of the 2026-08-14 session);
epoch-stamped events for the phase boundaries are in this file's git history
context and the session transcript.
