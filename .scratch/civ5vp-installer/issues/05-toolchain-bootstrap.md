# 05 — Toolchain Bootstrap: downloads + in-process SDK extraction

**What to build:** The first time a build is needed, the installer downloads the pinned portable LLVM and the Windows SDK 7.0 ISO from its pinned source with visible progress, extracts the SDK in-process (ISO9660 + MSI + CAB parsing — no wine, msitools, or 7-Zip on the user's machine), applies case-folding fixes on Linux, and caches everything in the Toolchain Cache. Every later build starts instantly from the cache. This resolves the spec's extraction-fidelity bet.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

**Every URL, checksum, ISO member and fix-up this ticket needs is recorded in `docs/pinned-artifacts.md`.** Read it first. Two things there override older prose: the VC9 CRT ships **inside the same Windows SDK ISO** (`Setup/vc_stdx86/`), so there is exactly one mandatory download, not two; and the proven compiler is **clang 18** targeting `i386-pc-windows-msvc`. Do not take build settings from upstream `master` — its Release configuration produces a DLL the game rejects.

- [x] SDK ISO download verifies against the recorded SHA-256 before extraction, is resumable or cleanly restartable, and reports progress
- [x] Portable LLVM 18.x is fetched and pinned; its version is part of the Toolchain identity and feeds the Build Fingerprint
- [x] In-process extraction pulls exactly the ISO members listed in `docs/pinned-artifacts.md`, honouring each MSI's CAB-name-to-real-path mapping rather than guessing
- [x] All six Linux fix-ups from `docs/pinned-artifacts.md` are applied (lowercase + backward symlinks, case-mismatched `#include` resolution, `Include`/`Lib` symlinks, backslash-to-slash in `#include` directives, per-`.lib` case symlinks, WDK header stubs)
- [ ] Extraction is verified against the docker image's known-good result: `windows.h`, `stdio.h`, `iostream`, `kernel32.lib`, `msvcrt.lib` and `DriverSpecs.h` all resolve, and header/lib counts match the committed reference baseline
- [x] Case-folding fixes applied so the headers/libs resolve on a case-sensitive filesystem
- [x] Bootstrap runs once; subsequent builds detect the populated Toolchain Cache and skip it
- [x] Interrupted bootstrap leaves a state that self-repairs on retry
- [x] Slow integration test (real downloads) exists but is excluded from the per-commit suite

## Comments

Landed as a new crate, `crates/toolchain` (`civ5vp-toolchain`), depending on `civ5vp-core` for
the `ToolchainRunner` boundary and on nothing UI-shaped. The Core keeps its zero dependencies.

### Three things `docs/pinned-artifacts.md` gets wrong about the ISO

All three were found by pointing the code at the real download; none is a code problem, all
three change what the document should say.

1. **The image is UDF, not ISO9660.** `GRMSDK_EN_DVD.iso` is an ISO-13346 (UDF) disc. Its
   ISO9660 side contains exactly one file, a `README.TXT` reading *"This disc contains a
   'UDF' file system and requires an operating system that supports the ISO-13346 'UDF' file
   system specification."* Everything the bootstrap needs is on the UDF side. ADR-0001 and
   the spec both say "ISO9660 + MSI + CAB parsing inside the installer"; an ISO9660-only
   installer cannot read this artifact at all. Both readers now exist (`src/udf.rs`,
   `src/iso9660.rs`) and `src/disc.rs` picks by probing for a UDF anchor.
2. **It is 1.45 GiB, not ~580 MB.** `Content-Length` from the pinned archive.org URL is
   1,552,508,928 bytes. With the 1.0 GB LLVM tarball a first bootstrap is ~2.4 GB, against
   ADR-0001's "~700 MB one-time download".
3. **The MSIs do not extract to a flat `Include/` and `Lib/`.** They place files where
   Windows would have installed them — `Program Files/Microsoft SDKs/Windows/v7.0/Include/…`,
   with `Lib/x64/` beside `Lib/`. Honouring the MSI mapping (which §1 requires) means the
   layout stays nested, so `src/sdk_layout.rs` *finds* the include and lib roots rather than
   assuming them, and `Toolchain::include_dirs()` / `lib_dirs()` hand them to ticket 06.

Measured from the real image: `Setup/WinSDK/cab1.cab` holds 120 files in one LZX folder;
`Setup/WinSDKBuild/cab1..4.cab` hold 641 / 1013 / 862 / 320 files, ~52 MB per folder, all
`Lzx(MB2)`. `WinSDKBuild_x86.msi` maps 2836 files across those four cabinets.

### Blocker for ticket 06: the pinned Linux LLVM does not run

Found by running it. `clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04` links against
`libtinfo.so.5`; no current distribution ships that (Arch, Fedora and Ubuntu 22.04+ all have
`libtinfo.so.6`), so the dynamic loader refuses to start `clang-18` and `lld` at all:

```text
./clang-18: error while loading shared libraries: libtinfo.so.5: cannot open shared object
file: No such file or directory
```

An `LD_LIBRARY_PATH` shim pointing `libtinfo.so.5` at `libtinfo.so.6` does not work — the
binaries want ncurses 5's versioned symbols. And this is the **only** x86-64 Linux build
llvm.org publishes for LLVM 18: 18.1.3, the version Ubuntu 24.04 ships and the one the
reference build actually uses, has no x86-64 Linux asset at all.

Everything around the pin is verified against this artifact and unaffected by changing it —
download, checksum, xz decode, tar, the keep-filter, the Toolchain identity. Swapping it is
one constant and one checksum in `pinned.rs`. Choosing what to swap it *for* is ADR-sized:

- **Ubuntu 24.04's own `.deb` packages** (`clang-18`, `lld-18`, `libclang-cpp18`, `libllvm18`
  and their dependencies). Closest to the proven configuration — §2 says the reference build
  takes clang 18 from exactly there — and `.deb` is `ar` + `tar.zst`, both reachable in pure
  Rust. The cost is resolving a small dependency web and pinning each package.
- **A third-party portable LLVM** built against a current glibc/ncurses.
- **Shipping a `libtinfo.so.5`** beside the toolchain. Smallest change, but it means pinning
  an ncurses 5 binary from somewhere and carrying its licence.

Until this is settled, ticket 06 cannot compile on Linux however good the SDK extraction is.

### Not ticked, and why

The docker-baseline box stays open. There is no docker image here and no reference
header/lib counts to compare against, so `verify::REFERENCE_BASELINE` is `None` and the
`#[ignore]`d integration test prints the counts it measured rather than asserting on them.
The half of that criterion that *is* satisfied — all six names from
`docs/pinned-artifacts.md` §4 resolving — is asserted unconditionally, in the fast suite
against synthetic fixtures. Whoever runs the reference container next should paste its two
numbers into `REFERENCE_BASELINE`; the comparison is already wired.

### Why the CAB reader is hand-rolled

`cab::Cabinet::read_file` rebuilds a folder reader per call and decompresses that folder from
its start every time, so extracting *N* files out of one folder costs O(N × folder size). Each
real cabinet is a single ~52 MB LZX folder holding hundreds of files, which works out at about
69 GB of decompression to extract 168 MB. `src/cabinet.rs` reads each folder once instead and
writes files as their bytes go past.

The decompression itself is still `flate2` (MSZIP) and `lzxd` (LZX) — the same crates `cab`
delegates to — and the reader is cross-checked rather than trusted: the fast suite round-trips
MSZIP cabinets written by `cab`'s own builder and compares both readers member for member, and
`inspect_a_real_disc_image` does the same on the real LZX cabinets. **33 members across five
real cabinets agree byte for byte**, and the whole cross-check runs in about four seconds.

### Other deliberate scope calls

- `docs/pinned-artifacts.md` §2 pins clang 18 but no URL, because the reference build
  apt-installs it. The pinned artifact is now the llvm.org release tarball for 18.1.8
  (Linux x86-64 and Windows x86-64). llvm.org publishes no checksums for those, so both
  SHA-256 values in `pinned.rs` were measured by downloading and hashing the assets. Worth a
  second pair of eyes: weaker provenance than the ISO's, whose checksum the document carries.
- The tarball expands to ~4.5 GB, most of it LLVM's own static libraries and headers, so
  `tarball.rs` keeps only `bin/`, `lib/clang/` and the shared libraries in `lib/`. The 1.0 GB
  download itself cannot be trimmed without finding a smaller portable clang 18 — an
  ADR-sized question.
- ISO9660, UDF and the CAB container are hand-rolled; MSI is not, and neither is any
  compression. `msi` (mdsteele, pure Rust) reads the installer databases and its *writer* half
  builds those fixtures, so they come from an implementation independent of the one under
  test. `cab` is now a dev-dependency only, for the same reason plus the cross-check.
- `BootstrappedToolchain::build_dll` bootstraps the Toolchain and then returns a typed error
  saying compilation is not implemented. That is ticket 06's work; emitting a stub DLL instead
  would let a broken install reach the game.
- The Windows LLVM checksum is pinned but **untested** — no Windows machine (`AGENTS.md`).
  The fix-ups are a documented no-op there, because NTFS already resolves every spelling and
  creating symlinks needs a privilege user story 34 says the installer must not require.

**Dependencies added,** each with its reason in the commit that adds it: `ureq` (rustls, no
OpenSSL), `sha2`, `msi`, `flate2`, `lzxd`, `tar`, `lzma-rs`, and `cab` as a dev-dependency.
All pure Rust; none pulls a C toolchain.

**Tests.** 91 unit tests inside `crates/toolchain`, plus three `#[ignore]`d ones: two in
`crates/toolchain/tests/real_bootstrap.rs` (the real download and extraction, and the
cache-reuse check) and `extract::tests::inspect_a_real_disc_image`, which describes a real
image without extracting it and is what found all three documentation errors above. The fast
suite never opens a socket: the whole bootstrap sequence runs against a synthetic UDF image
built byte by byte in-process, containing four real MSIs over seven real CABs.
