# Pinned Artifacts

Everything the installer downloads from the internet, with the exact URLs and the checks that prove the download is the right one. Nothing else may be fetched at runtime.

**Source of truth:** the `docker` branch of **`Alpakinator/Community-Patch-DLL`** — <https://github.com/Alpakinator/Community-Patch-DLL/tree/docker>.

> **Do not take build settings from upstream `master` or any other branch.** Only the `docker` branch's clang configuration produces a DLL the game accepts; the Release configuration on other branches is wrong and yields a broken DLL. Upstream `LoneGazebo/Community-Patch-DLL` has no `docker` branch at all — `build_vp_clang.py` and `build_vp_clang_sdk.py` exist on its `master`, but the Linux build (`build_vp_clang_linux.py`, `Dockerfile`, `setup_sdk.sh`, `fix_lib_case.sh`, `docker-build.sh`, `scripts/deploy_ingame.py`) exists only on the fork's `docker` branch. This is what ADR-0001 and the spec mean by "the docker-branch build".

## 1. Windows SDK 7.0 ISO — the only mandatory download

| | |
| --- | --- |
| **URL** | `https://web.archive.org/web/20161230154527/http://download.microsoft.com/download/2/E/9/2E911956-F90F-4BFB-8231-E292A7B6F287/GRMSDK_EN_DVD.iso` |
| **Size** | **1.45 GiB** (1,552,508,928 bytes) |
| **Filesystem** | **UDF**, not ISO9660 — see below |
| **SHA-256** | `65739fb0874cc17ea6962d8ce7915364c7161fa106ed1bf1c917924c18ac63ca` |
| **Provenance** | URL and hash copied verbatim from the docker branch's `Dockerfile` (`SDK_URL`, and the `sha256sum -c` beside it) — verified identical |
| **Provides** | Windows SDK 7.0 headers + import libs **and** the VC9 CRT |

**This single ISO covers both halves of the toolchain.** The VC9 CRT is not a separate download — it lives inside the ISO at `Setup/vc_stdx86/`. Earlier prose describing the CRT as coming from a Visual Studio ISO is wrong for the Linux path.

Only these members are needed (everything else in the ISO is ignored):

```
Setup/WinSDK/WinSDK_x86.msi                     + cab1.cab
Setup/WinSDKBuild/WinSDKBuild_x86.msi           + cab1.cab .. cab4.cab   (headers + import libs)
Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi + cab1.cab
Setup/vc_stdx86/vc_stdx86.msi                   + vc_stdx86.cab          (VC9 CRT)
```

Each MSI carries the mapping from CAB-internal names to real file paths — that mapping must be honoured, not guessed. The reference implementation shells out to `7z` and `msiextract`; **ours parses UDF, MSI, and CAB in-process** (ADR-0001), so this list is the extraction contract to verify against.

### Corrections, measured against the real download (ticket 05)

Three things this document originally said were wrong. All three were found by pointing code at
the actual artifact, and two are independently checkable with a single ranged request.

**1. The image is UDF, not ISO9660.** Its volume recognition sequence reads:

| sector | descriptor |
| --- | --- |
| 16 | `CD001` — the ISO9660 primary volume descriptor |
| 17 | `CD001`, type 255 — terminator |
| 18 | `BEA01` — beginning of the extended area |
| 19 | `NSR02` — ISO 13346 / UDF |

It is a *bridge* disc: the ISO9660 side exists but holds a single `README.TXT` saying the
content is in the UDF filesystem. An ISO9660-only reader — which is what ADR-0001 and the spec
originally specified — cannot read a single one of the members listed above. The installer
therefore carries a UDF reader and probes for the anchor to decide which to use.

**2. It is 1.45 GiB, not ~580 MB.** `Content-Length` from the pinned URL is 1,552,508,928 bytes.
With the portable LLVM alongside it, a first bootstrap moves roughly 2.4 GB, not the "~700 MB"
ADR-0001 estimated. This is a user-visible number — it belongs in the progress UI and in
whatever the storage panel says about the ~5 GB footprint.

**3. The MSIs do not extract to a flat `Include/` and `Lib/`.** The real paths are nested under
`Program Files/Microsoft SDKs/Windows/v7.0/`. Extraction locates the roots rather than assuming
them, and hands them to the build as explicit include and lib directories.

### Why the download is slow, and the mirror that is *not* a substitute

The pinned URL is a Wayback Machine capture, and Wayback is a replay service rather than a file
server: it locates the record in WARC storage and streams it back out. Measured on one machine
within the same minute:

| source | throughput | time to first byte |
| --- | --- | --- |
| GitHub (for comparison) | 8.4 MB/s | immediate |
| `web.archive.org/…/` replay | 0.20 MB/s | 12.3 s |
| `web.archive.org/…id_/` raw | 0.22 MB/s | 7.4 s |

So a first bootstrap spends a couple of hours on this file, and that is the artifact's fault,
not the network's or the installer's. A `Range` request into the middle of it returned **zero
bytes in 20 seconds**, which is why resuming is unreliable — the downloader must tolerate that.
The original `download.microsoft.com` URL is a hard 404, so there is no first-party source left.

**Do not swap the pin for `archive.org/download/grmsdkx-en-dvd/GRMSDK_EN_DVD.iso`.** It is
tempting: it is a real archive.org item rather than a Wayback capture, it serves at 7.7 MB/s,
and it has the same file name. It is a **different product** — the item is "Windows SDK June
2010 (for Windows 7 & .NET Framework 4)", which is SDK **7.1**, not the 7.0 this project pins.
Measured: 594,841,600 bytes, SHA-256 `27cb38f76095c0acb9b558109f8693b39cbceb796856c15bd575f9e9d0b316c3`
— which is not the SHA-256 above, so the checksum gate would catch the swap. Recorded here so
the next person can see *why* it fails instead of assuming the hash is stale.

(That collision is the likely source of this document's original "~580 MB": SDK 7.1's ISOs are
about that size, SDK 7.0's is 1.45 GiB.)

Searching archive.org for a faster 7.0 mirror turns up four items, none of which is one.

To re-check the first two without downloading the whole image:

```sh
URL='https://web.archive.org/web/20161230154527/http://download.microsoft.com/download/2/E/9/2E911956-F90F-4BFB-8231-E292A7B6F287/GRMSDK_EN_DVD.iso'
curl -sIL "$URL" | grep -i content-length          # 1552508928
curl -s -r 32768-40959 -L "$URL" | xxd | grep -E 'CD001|BEA01|NSR0'
```

## 2. Compiler

The reference build uses **clang 18 with lld**, from Ubuntu 24.04's default repositories, targeting **`i386-pc-windows-msvc`** (Win32 x86). `clang-cl` is a symlink to `clang`.

The installer cannot apt-install anything, so Toolchain Bootstrap must fetch an equivalent **portable LLVM 18.x** and pin it. The proven configuration is clang 18 — treat the major version as part of the pinned toolchain identity, not an incidental detail, and record it in the Build Fingerprint.

### This is the one pin with no counterpart upstream

Everything else here is copied from the docker branch. This is not, and cannot be: the reference
build is `FROM ubuntu:24.04` plus `apt-get install clang lld`, so it has **no compiler download
URL at all**. Whatever we pin is our own substitute for a distribution package, and the
substitution is where fidelity is lost:

* Ubuntu 24.04 ships **clang 18.1.3**. llvm.org publishes **no x86-64 Linux asset for 18.1.3**,
  so an exact match to the reference is not available as a portable build.
* The nearest llvm.org release, **18.1.8 `x86_64-linux-gnu-ubuntu-18.04`**, is currently pinned
  and **does not run**: it links `libtinfo.so.5`, which no current distribution ships, so the
  loader refuses to start `clang-18` and `lld`. An `LD_LIBRARY_PATH` shim onto `libtinfo.so.6`
  fails on ncurses 5's versioned symbols.

So the compiler pin is an open question, and it blocks ticket 06 rather than ticket 05 —
everything around it (download, checksum, xz decode, tar, keep-filter, Toolchain identity) is
verified against the artifact and unaffected by changing it. Swapping it is one constant and one
checksum; choosing the replacement deserves an ADR. The closest-to-proven option is Ubuntu
24.04's own `clang-18` / `lld-18` `.deb` packages, which are exactly what the reference build
installs and are reachable with pure-Rust `ar` + `tar.zst`.

## 3. Post-extraction fix-ups (Linux)

The extracted SDK does not work as-is on a case-sensitive filesystem. All of these are part of a correct Toolchain Bootstrap:

1. **Lowercase every filename under `Include/`**, leaving a symlink from the original mixed-case name to the lowercase one.
2. **Resolve case-mismatched `#include "X"` directives** — where a header includes a name that differs only in case from the file on disk, add a symlink.
3. **Symlink `Include/` and `Lib/`** to their lowercase forms if the extraction produced lowercase directories; the build expects the capitalised names.
4. **Rewrite backslashes to forward slashes** in `#include` directives inside SDK headers — the SDK uses Windows path separators that Linux reads literally.
5. **Case symlinks for every `.lib`** — the SDK and CRT mix `GDI32.lib`, `Kernel32.Lib`, etc., and the linker may reference either case.
6. **Stub the WDK-only headers** referenced by the SDK but shipped only with the Driver Kit: `DriverSpecs.h` and `SpecStrings.h` (plus lowercase variants). Empty files satisfy the `#include` chain for user-mode code.

## 4. Extraction verification

A bootstrap is only complete when all of these resolve under the toolchain root:

`windows.h` · `stdio.h` · `iostream` · `kernel32.lib` · `msvcrt.lib` · `DriverSpecs.h`

Ticket 05's "layout equivalent to the docker image's known-good extraction" means: same header count, same lib count, and every name above resolving. Capture the reference counts from a real docker build once and commit them as the comparison baseline.

## 5. Not downloaded by the Linux build

These appear in the reference repo's documentation for the **Windows / Visual Studio** path. The installer does not fetch them, and they should not be added without an ADR:

- `VS2008ExpressWithSP1ENUX1504728.iso` (VC9 via Visual Studio)
- `VS2010Express1.iso`
- `VS10SP1-KB2736182.exe` (VS2010 SP1 compiler update)

## 6. Mod sources

The Community Patch / Vox Populi sources come from the Upstream Cache — an incremental clone of `LoneGazebo/Community-Patch-DLL` (~4.5 GB of history; see ticket 04 for the transfer budget) — or from a Local Repo. No other network source exists.
