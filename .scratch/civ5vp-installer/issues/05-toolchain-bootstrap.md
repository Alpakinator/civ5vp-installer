# 05 — Toolchain Bootstrap: downloads + in-process SDK extraction

**What to build:** The first time a build is needed, the installer downloads the pinned portable LLVM and the Windows SDK 7.0 ISO from its pinned source with visible progress, extracts the SDK in-process (ISO9660 + MSI + CAB parsing — no wine, msitools, or 7-Zip on the user's machine), applies case-folding fixes on Linux, and caches everything in the Toolchain Cache. Every later build starts instantly from the cache. This resolves the spec's extraction-fidelity bet.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

**Every URL, checksum, ISO member and fix-up this ticket needs is recorded in `docs/pinned-artifacts.md`.** Read it first. Two things there override older prose: the VC9 CRT ships **inside the same Windows SDK ISO** (`Setup/vc_stdx86/`), so there is exactly one mandatory download, not two; and the proven compiler is **clang 18** targeting `i386-pc-windows-msvc`. Do not take build settings from upstream `master` — its Release configuration produces a DLL the game rejects.

- [x] SDK ISO download verifies against the recorded SHA-256 before extraction, is resumable or cleanly restartable, and reports progress
- [x] Portable LLVM 18.x is fetched and pinned; its version is part of the Toolchain identity and feeds the Build Fingerprint
- [x] In-process extraction pulls exactly the ISO members listed in `docs/pinned-artifacts.md`, honouring each MSI's CAB-name-to-real-path mapping rather than guessing
- [x] All six Linux fix-ups from `docs/pinned-artifacts.md` are applied (lowercase + backward symlinks, case-mismatched `#include` resolution, `Include`/`Lib` symlinks, backslash-to-slash in `#include` directives, per-`.lib` case symlinks, WDK header stubs)
- [x] Extraction is verified against the docker image's known-good result: `windows.h`, `stdio.h`, `iostream`, `kernel32.lib`, `msvcrt.lib` and `DriverSpecs.h` all resolve, and header/lib counts match the committed reference baseline — **with one qualifier: the baseline is measured by this implementation, not read off a docker image. See Comments.**
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

### What is and is not proven

**The extraction-fidelity bet is resolved.** The end-to-end run against the real image
completed: SHA-256 matched the pinned `65739fb0…` exactly, 3660 files extracted in 107 s, all six
fix-ups ran on the real tree, and all six of §4's names resolve. Independently re-checked against
the extracted tree afterwards — 2033 headers and 928 libraries, reproduced with a `find` using
the same predicate as `verify.rs`.

One qualifier on the sixth name, which the review caught: **`DriverSpecs.h` cannot fail.**
Fix-up 6 writes it (a 58-byte stub — it is a WDK-only header the SDK includes but does not ship)
into every include root immediately before verification runs, so its presence is self-fulfilling
rather than evidence. Five of the six names are real evidence; that one is a check on our own
output. Worth remembering if §4 is ever treated as an independent test.

What *is* proven against real data, which is not nothing:

- the UDF reader reads the real image's volume structures and directory tree;
- the MSI parser reads the real `WinSDK_x86.msi` (120 files) and `WinSDKBuild_x86.msi`
  (2836 files across four cabinets);
- the CAB reader agrees with the `cab` crate byte-for-byte on 33 members across five real LZX
  cabinets.

So the readers are validated on real bytes and the orchestration is not. The remaining work is
to run `real_bootstrap.rs` to completion — the test exists and is runnable.

**One open risk to check when it runs.** `iostream` and `msvcrt.lib` must come from
`Setup/vc_stdx86/`, which lies past the bytes downloaded so far. If that MSI ships only the
redistributable DLLs, the verification list in `docs/pinned-artifacts.md` §4 is not achievable
from the pinned members and §4 itself needs revisiting.

**LLVM checksum provenance is weaker than the ISO's.** llvm.org publishes no checksums, so both
were measured by downloading. The Windows one has not been verified on a Windows machine.

### Blocker for ticket 06: the pinned Linux LLVM does not run

**The SDK half is finished and proven; this is the only thing standing between ticket 06 and
a compile.**

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

### The real extraction, and what the baseline is worth

The pinned image was downloaded, extracted and verified end to end on 2026-08-03. Its
SHA-256 matches the recorded `65739fb0…c18ac63ca` exactly, so the document's checksum is
right even though its size is not.

```text
3660 files unpacked (567 MB) in 107 s
2033 headers, 928 import libraries
fix-ups: 831 lowercased, 63 include case-links, 4 directory links,
         7 backslash rewrites, 5777 lib case-links, 6 WDK stubs
```

All six names from §4 resolve, and where they came from settles the document's claim that one
ISO carries both halves of the toolchain:

| Name | Where it landed |
| --- | --- |
| `windows.h` | `…/Microsoft SDKs/Windows/v7.0/Include/` |
| `kernel32.lib` | `…/Microsoft SDKs/Windows/v7.0/Lib/` |
| `stdio.h` | `…/Microsoft Visual Studio 9.0/VC/include/` |
| `iostream` | `…/Microsoft Visual Studio 9.0/VC/include/` |
| `msvcrt.lib` | `…/Microsoft Visual Studio 9.0/VC/lib/` |
| `DriverSpecs.h` | `…/Microsoft Visual Studio 9.0/VC/include/` (stubbed by fix-up 6) |

A second `ensure` on the populated cache returns in **36 µs**.

**The qualifier on the ticked box.** `verify::REFERENCE_BASELINE` now holds 2033 / 928, and
the `#[ignore]`d test asserts against it — but those are *our* numbers, not a docker image's.
That makes it a regression guard on this extraction, which catches a reader or fix-up change
that silently drops files. It is not the cross-check against a known-good build the document
asks for. If someone runs the reference container and the counts differ, ours is the one that
is wrong.

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

**Tests.** 93 unit tests inside `crates/toolchain`, plus four `#[ignore]`d ones: two in
`crates/toolchain/tests/real_bootstrap.rs` (the real download and extraction, and the
cache-reuse check), `extract::tests::inspect_a_real_disc_image`, which describes a real image
without extracting it and is what found the documentation errors above, and
`tarball::tests::unpacks_a_real_llvm_release`, which decodes a real 1 GB xz stream — the one
thing the kilobyte-sized fixtures cannot reach. The fast
suite never opens a socket: the whole bootstrap sequence runs against a synthetic UDF image
built byte by byte in-process, containing four real MSIs over seven real CABs.

## Proven end to end — and a fix-up bug found by doing it

Compiled a real Win32 object with the bootstrapped toolchain: C++ including `<windows.h>` and
`<string>`, through `clang-cl --target=i386-pc-windows-msvc` with the reference build's flags,
against the SDK this installer extracted. Output: `Intel i386 COFF object file, 8 sections`.
`lld-link` runs too. So the extracted SDK is not merely *present*, it is *usable*.

Two things had to be true, and only one of them was.

### 1. The compiler blocker is solved by bundling one small library

The pinned llvm.org build needs `libtinfo.so.5` only to ask the terminal whether it supports
colour. Dropping a real ncurses-5 `libtinfo.so.5` (187 KB, from Ubuntu's `libtinfo5` package)
beside it and setting `LD_LIBRARY_PATH` makes it run:

```
without:  error while loading shared libraries: libtinfo.so.5
with:     clang version 18.1.8
```

The earlier attempt failed because it *aliased* `libtinfo.so.5` to the system's `libtinfo.so.6`;
ncurses 5 and 6 export incompatible versioned symbols. Shipping the real ncurses-5 library is a
different thing and works. This keeps the most portable compiler available: the llvm.org build
requires only **glibc 2.27**, against **2.34+** for a distro-built binary — measured with
`objdump -T`. For an installer shipped to players on arbitrary distributions that difference
matters more than matching the reference's point release.

### 2. Fix-up 6 shadows real SDK headers — a defect

`fixups.rs` writes empty 58-byte stubs for `DriverSpecs.h`, `SpecStrings.h` (both cases) into
**every** include root. `docs/pinned-artifacts.md` §3 item 6 calls these "WDK-only headers…
shipped only with the Driver Kit", but that is wrong: **the SDK ships them**, as
`driverspecs.h` (31 KB) and `specstrings.h` (23 KB). The stubs land beside the real files and
win, for two compounding reasons:

* `kernelspecs.h:33` uses a **quoted** include, `#include "DriverSpecs.h"`, which searches its
  own directory first — so the stub next to it always wins, whatever `-I` order is used;
* stubs are also written into the **VC9** include directory, which is searched before the SDK's,
  so `<specstrings.h>` resolves to a 58-byte file globally.

The result is that `__ANNOTATION` is never defined and `windows.h` cannot be included at all.
Verified: `#include <specstrings.h>` followed by `#ifdef __ANNOTATION` reports it **missing**.

Replacing the two SDK stubs with symlinks to the real lowercase headers, and dropping the four
from the VC9 include directory, is what made the compile above succeed. `DelayImp.h` and
`Warnings.h` stubs are fine and were left alone — VC9 genuinely does not ship those.

**The rule fix-up 6 needs:** stub a header only when no case-variant of it exists anywhere on
the include path. Where one does exist, fix-up 2's case symlink is the correct answer.

**This is also why §4's `DriverSpecs.h` check is worse than useless.** It does not merely fail to
prove anything — it reported success against the very stub that was breaking the build.

## Implemented: the compiler runs, and the toolchain builds a DLL

ADR-0005 and the fix-up 6 correction are in code now, and the result is the thing this ticket
was for. From a cache the installer bootstrapped itself, with no manual steps and no environment
variables set:

```
clang version 18.1.8            lld-link: LLD 18.1.8
compile → Intel i386 COFF object file, 8 sections
link    → PE32 executable for MS Windows 6.00 (DLL), Intel i386, 4 sections
```

A real Windows DLL, from a compiler and an SDK this installer downloaded, verified, extracted
from a UDF image and fixed up by itself. **Ticket 06 is unblocked.**

Two changes got it there.

**`crates/toolchain/src/deb.rs`** reads one file out of a Debian package: `ar` parsed by hand
(the format is a magic string and 60-byte text headers — less code than justifying a dependency
would be), then the existing `lzma-rs` for `data.tar.xz`, then `tar`. The library is written into
the compiler's own `lib/`, where the llvm.org binaries' existing `RUNPATH: $ORIGIN/../lib` finds
it — so no `LD_LIBRARY_PATH` has to be arranged around every compiler invocation. No new
dependency.

**Fix-up 6 now asks whether the SDK ships a header before stubbing it**, across every include
root rather than per-root — the CRT's directory is searched first, so a stub written there
shadows the SDK's copy globally. See the header comment on `stub_wdk_headers` for the full
account.

### The baseline moved, and that is the guard working

The header count went 2033 → 2027. The difference is exactly the six stubs fix-up 6 no longer
writes, and losing them is what made `windows.h` includable. The `#[ignore]`d test refused the
run until the committed baseline was updated — which is precisely what a regression guard on our
own extraction is for.

### Also found

The two `#[ignore]`d tests share one Toolchain Cache and cannot run in parallel; they clobber
each other's staging directory mid-cabinet. `--test-threads 1` is now in the AGENTS.md command.
