# Pinned Artifacts

Everything the installer downloads from the internet, with the exact URLs and the checks that prove the download is the right one. Nothing else may be fetched at runtime.

**Source of truth:** the `docker` branch of **`Alpakinator/Community-Patch-DLL`** - <https://github.com/Alpakinator/Community-Patch-DLL/tree/docker>.

> **Do not take build settings from upstream `master` or any other branch.** Only the `docker` branch's clang configuration produces a DLL the game accepts; the Release configuration on other branches is wrong and yields a broken DLL. Upstream `LoneGazebo/Community-Patch-DLL` has no `docker` branch at all - `build_vp_clang.py` and `build_vp_clang_sdk.py` exist on its `master`, but the Linux build (`build_vp_clang_linux.py`, `Dockerfile`, `setup_sdk.sh`, `fix_lib_case.sh`, `docker-build.sh`, `scripts/deploy_ingame.py`) exists only on the fork's `docker` branch. This is what ADR-0001 and the spec mean by "the docker-branch build".

## 1. Windows SDK 7.0 ISO - the only mandatory download

| | |
| --- | --- |
| **URL** | `https://web.archive.org/web/20161230154527/http://download.microsoft.com/download/2/E/9/2E911956-F90F-4BFB-8231-E292A7B6F287/GRMSDK_EN_DVD.iso` |
| **Size** | **1.45 GiB** (1,552,508,928 bytes) |
| **Filesystem** | **UDF**, not ISO9660 - see below |
| **SHA-256** | `65739fb0874cc17ea6962d8ce7915364c7161fa106ed1bf1c917924c18ac63ca` |
| **Provenance** | URL and hash copied verbatim from the docker branch's `Dockerfile` (`SDK_URL`, and the `sha256sum -c` beside it) - verified identical |
| **Provides** | Windows SDK 7.0 headers + import libs **and** the VC9 CRT |

**This single ISO covers both halves of the toolchain.** The VC9 CRT is not a separate download - it lives inside the ISO at `Setup/vc_stdx86/`. Earlier prose describing the CRT as coming from a Visual Studio ISO is wrong for the Linux path.

Only these members are needed (everything else in the ISO is ignored):

```
Setup/WinSDK/WinSDK_x86.msi                     + cab1.cab
Setup/WinSDKBuild/WinSDKBuild_x86.msi           + cab1.cab .. cab4.cab   (headers + import libs)
Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi + cab1.cab
Setup/vc_stdx86/vc_stdx86.msi                   + vc_stdx86.cab          (VC9 CRT)
```

Each MSI carries the mapping from CAB-internal names to real file paths - that mapping must be honoured, not guessed. The reference implementation shells out to `7z` and `msiextract`; **ours parses UDF, MSI, and CAB in-process** (ADR-0001), so this list is the extraction contract to verify against.

### The installer downloads the members, not the image

Those eleven files are **106,487,437 bytes - 6.9% of the image**. The rest is samples,
documentation, debuggers and the amd64/ia64 halves, none of which is ever read. Since the only
surviving source serves the image at a fraction of a megabyte a second (see below), fetching
all of it means spending hours on bytes nothing looks at.

So each member is fetched as its own windowed download: same URL, a `Range` over that member's
own bytes, checked against that member's own SHA-256. The offsets and hashes are part of the
pin. They were measured off a copy of the image whose whole-file SHA-256 matches the one above:

```bash
CIV5VP_SDK_ISO=/path/to/GRMSDK_EN_DVD.iso \
  cargo test --release -p civ5vp-toolchain --lib -- --ignored --nocapture describe_the_pinned_members
```

| member | offset | bytes | sha256 |
| --- | ---: | ---: | --- |
| `Setup/WinSDK/WinSDK_x86.msi` | 2,975,744 | 2,374,144 | `1e22e5f4c7324a77088d33aa9d3f555c85a525639e14a624b033ded032fd4da8` |
| `Setup/WinSDK/cab1.cab` | 5,351,424 | 29,986,366 | `c5076c9cd324161ec6e8fbf893be0aab75dd1c68866cdc93d6c043d2d69dd063` |
| `Setup/WinSDKBuild/WinSDKBuild_x86.msi` | 76,539,904 | 979,968 | `f1268291829854745ed2ab80e854c8a8514e876e71e3f810fdc06f40ca97edc3` |
| `Setup/WinSDKBuild/cab1.cab` | 77,520,896 | 5,742,456 | `bd2b525187d30f1d7cf7132cab080e212ec33f248226b4b5a6aa2a02ef0cf6ba` |
| `Setup/WinSDKBuild/cab2.cab` | 83,263,488 | 6,193,571 | `baa12eca0e63a31f3d9d5eddd0003a26bf7e49e69373eddc882cc74d6b8573c4` |
| `Setup/WinSDKBuild/cab3.cab` | 89,458,688 | 4,789,860 | `e69199e1c281838ecc70263296f5cc4b3a569cb1bf7bcfdb93775d8696264b33` |
| `Setup/WinSDKBuild/cab4.cab` | 94,248,960 | 1,020,063 | `c4635d2eae946c088b84d6c1e8874c5ade865440e9ac8325e472e529f28ed9e6` |
| `Setup/WinSDKWin32Tools/WinSDKWin32Tools_x86.msi` | 1,444,061,184 | 766,976 | `ba17c91c2fbdc09cf23ad126a489b6cd7b38ae2ff7098cc748348bf4bbe895f7` |
| `Setup/WinSDKWin32Tools/cab1.cab` | 1,444,829,184 | 10,896,725 | `551a7ccea577f8d2fea8a059e463e9e3ead1d9a03b73aa26b2886b8647ec8b29` |
| `Setup/vc_stdx86/vc_stdx86.msi` | 1,548,064,768 | 408,576 | `0a524433918357e8476fbf0191ad3a7fb45fad11ec929f5bd69b0d2713306ade` |
| `Setup/vc_stdx86/vc_stdx86.cab` | 1,504,735,232 | 43,328,732 | `d91cdb54fe5b4328b811b3b0bdd0b660a84b09e87cdce4da7d07ead63069e192` |

Each member is one unbroken run of bytes, which is what makes a single ranged request enough
for it; the measuring test asserts that, so re-measuring against a differently-mastered image
fails loudly instead of quietly fetching nonsense. A wrong offset cannot pass silently
either - the bytes would not match the member's SHA-256, and the download is refused.

The whole-image SHA-256 above is still the pin of record. An image downloaded by a version of
the installer that needed it (up to 0.1.2) is not re-fetched and not left to rot either: the
members are read out of it locally, checked, and the image is then deleted - 1.35 GiB of a
player's disk given back on the next install. A failed image download's `.part` and its ledger
go with it, since nothing asks for the whole image any more.

### Corrections, measured against the real download (ticket 05)

Three things this document originally said were wrong. All three were found by pointing code at
the actual artifact, and two are independently checkable with a single ranged request.

**1. The image is UDF, not ISO9660.** Its volume recognition sequence reads:

| sector | descriptor |
| --- | --- |
| 16 | `CD001` - the ISO9660 primary volume descriptor |
| 17 | `CD001`, type 255 - terminator |
| 18 | `BEA01` - beginning of the extended area |
| 19 | `NSR02` - ISO 13346 / UDF |

It is a *bridge* disc: the ISO9660 side exists but holds a single `README.TXT` saying the
content is in the UDF filesystem. An ISO9660-only reader - which is what ADR-0001 and the spec
originally specified - cannot read a single one of the members listed above. The installer
therefore carries a UDF reader and probes for the anchor to decide which to use.

**2. It is 1.45 GiB, not ~580 MB.** `Content-Length` from the pinned URL is 1,552,508,928 bytes.
With the portable LLVM alongside it, a first bootstrap moves roughly 2.4 GB, not the "~700 MB"
ADR-0001 estimated. This is a user-visible number - it belongs in the progress UI and in
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
bytes in 20 seconds**, which is why resuming is unreliable - the downloader must tolerate that.
The original `download.microsoft.com` URL is a hard 404, so there is no first-party source left.

**Do not swap the pin for `archive.org/download/grmsdkx-en-dvd/GRMSDK_EN_DVD.iso`.** It is
tempting: it is a real archive.org item rather than a Wayback capture, it serves at 7.7 MB/s,
and it has the same file name. It is a **different product** - the item is "Windows SDK June
2010 (for Windows 7 & .NET Framework 4)", which is SDK **7.1**, not the 7.0 this project pins.
Measured: 594,841,600 bytes, SHA-256
`27cb38f76095c0acb9b558109f8693b39cbceb796856c15bd575f9e9d0b316c3` - which is not the SHA-256
above, so the checksum gate would catch the swap. Recorded here so the next person can see
*why* it fails instead of assuming the hash is stale.

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

The installer cannot apt-install anything, so Toolchain Bootstrap must fetch an equivalent **portable LLVM 18.x** and pin it. The proven configuration is clang 18 - treat the major version as part of the pinned toolchain identity, not an incidental detail, and record it in the Build Fingerprint.

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

So the compiler pin is an open question, and it blocks ticket 06 rather than ticket 05 -
everything around it (download, checksum, xz decode, tar, keep-filter, Toolchain identity) is
verified against the artifact and unaffected by changing it. Swapping it is one constant and one
checksum; choosing the replacement deserves an ADR. The closest-to-proven option is Ubuntu
24.04's own `clang-18` / `lld-18` `.deb` packages, which are exactly what the reference build
installs and are reachable with pure-Rust `ar` + `tar.zst`.

## 2b. `libtinfo.so.5` - required for the portable LLVM to start (Linux only)

The llvm.org build links `libtinfo.so.5` (ncurses 5), which no current distribution ships, so
`clang` and `lld` refuse to start. One shared library is extracted from a pinned Debian package
into the toolchain's own `lib/`, where the binaries' existing `RUNPATH: $ORIGIN/../lib` finds
it - no `LD_LIBRARY_PATH`. Reasoning, alternatives and measurements in **ADR-0005**.

| | |
| --- | --- |
| **URL** | `http://deb.debian.org/debian/pool/main/n/ncurses/libtinfo5_6.2+20201114-2+deb11u2_amd64.deb` |
| **Size** | 336,728 bytes |
| **SHA-256** | `69e131ce3f790a892ca1b0ae3bfad8659daa2051495397eee1b627d9783a6797` |
| **Wanted member** | `lib/x86_64-linux-gnu/libtinfo.so.5.9` (191,928 bytes) |
| **Installed as** | `llvm-<version>/lib/libtinfo.so.5.9` + a `libtinfo.so.5` symlink |
| **Licence** | MIT/X11 (ncurses) |

Debian's package rather than Ubuntu's because its `data.tar.xz` is readable with `lzma-rs`, which
the toolchain crate already uses; Ubuntu's is `data.tar.zst`. A `.deb` is an `ar` archive, parsed
in-process like everything else here. Not needed on Windows.

## 3. Post-extraction fix-ups (Linux)

The extracted SDK does not work as-is on a case-sensitive filesystem. All of these are part of a correct Toolchain Bootstrap:

1. **Lowercase every filename under `Include/`**, leaving a symlink from the original mixed-case name to the lowercase one.
2. **Resolve case-mismatched `#include "X"` directives** - where a header includes a name that differs only in case from the file on disk, add a symlink.
3. **Symlink `Include/` and `Lib/`** to their lowercase forms if the extraction produced lowercase directories; the build expects the capitalised names.
4. **Rewrite backslashes to forward slashes** in `#include` directives inside SDK headers - the SDK uses Windows path separators that Linux reads literally.
5. **Case symlinks for every `.lib`** - the SDK and CRT mix `GDI32.lib`, `Kernel32.Lib`, etc., and the linker may reference either case.
6. **Stub headers the SDK references but does not ship.** Empty files satisfy the `#include` chain for user-mode code. Applied only where *no case-variant of the name exists anywhere on the include path* - see the correction below.

   > **Correction.** This item previously named `DriverSpecs.h` and `SpecStrings.h` as WDK-only. **They are not - the SDK ships both**, as `driverspecs.h` (31 KB) and `specstrings.h` (23 KB). Stubbing them is actively harmful: `kernelspecs.h` includes `"DriverSpecs.h"` with *quotes*, so the stub beside it wins regardless of `-I` order, and stubs written into the VC9 include directory shadow the SDK's copies globally. `__ANNOTATION` then never gets defined and `windows.h` cannot be included at all. The rule is: **stub only when no case-variant of the header exists anywhere on the include path**; where one does, fix-up 2's case symlink is the correct answer.

## 4. Extraction verification

A bootstrap is only complete when all of these resolve under the toolchain root:

`windows.h` · `stdio.h` · `iostream` · `kernel32.lib` · `msvcrt.lib` · `DriverSpecs.h`

Ticket 05's "layout equivalent to the docker image's known-good extraction" means: same header count, same lib count, and every name above resolving. Capture the reference counts from a real docker build once and commit them as the comparison baseline.

**Measured by this installer on 2026-08-03**, extracting the real image (SHA-256 verified against
the value above): **2033 headers, 928 import libraries**, 3660 files in 107 s. All six names
resolve, and where they come from confirms §1's claim that one ISO covers both halves of the
toolchain - `windows.h` and `kernel32.lib` from `Microsoft SDKs/Windows/v7.0/`, and `stdio.h`,
`iostream` and `msvcrt.lib` from `Microsoft Visual Studio 9.0/VC/`. These are *our* numbers, not
a docker image's; treat them as a regression baseline, not as independent confirmation.

**`DriverSpecs.h` should be removed from this list.** The SDK ships it; fix-up 6 was wrongly
stubbing it, and the check then reported success against the very stub that made `windows.h`
unusable. A verification that passes *because* of a bug is worse than no verification. The other
five names are genuine. Replace it with the check that actually matters: compiling a translation
unit that includes `<windows.h>`.

## 5. Not downloaded by the Linux build

These appear in the reference repo's documentation for the **Windows / Visual Studio** path. The installer does not fetch them, and they should not be added without an ADR:

- `VS2008ExpressWithSP1ENUX1504728.iso` (VC9 via Visual Studio)
- `VS2010Express1.iso`
- `VS10SP1-KB2736182.exe` (VS2010 SP1 compiler update)

## 6. Mod sources

The Community Patch / Vox Populi sources come from the Upstream Cache - an incremental clone of `LoneGazebo/Community-Patch-DLL` (~4.5 GB of history; see ticket 04 for the transfer budget) - or from a Local Repo. No other network source exists.

## 7. LuaJIT - the replacement Lua engine (only when opted into)

Fetched and built only when the player turns the LuaJIT engine on; a stock-engine install never
touches it. Reasoning in **ADR-0006**.

| | |
| --- | --- |
| **Repository** | `https://github.com/LuaJIT/LuaJIT.git` |
| **Commit** | `1edc3e52b67eaf6ce5f809be8e17d6862594b8bc` (branch `v2.1`) |
| **Built as** | a 32-bit `i386-pc-windows-msvc` DLL, deployed as `lua51_Win32.dll` |
| **Never built with** | `LUAJIT_ENABLE_LUA52COMPAT` |
| **Licence** | MIT - redistributable |

Pinned by commit rather than by tag or tarball, which is a stronger pin than anything above:
GitHub's generated archives are not byte-stable, so their checksums cannot be relied on, and
`v2.1` is a *branch* that moves. A commit SHA needs no separate checksum - it is the content
check.

`LUAJIT_ENABLE_LUA52COMPAT` is listed here rather than left to the build code because it is a pin
in the same sense the others are: Civilization V and Vox Populi are written against Lua 5.1, and
5.2 semantics would only add divergence from the engine the scripts were tested on.

### The engine is kept, not rebuilt

What comes out of this build is a function of three things - the pinned commit, the patches
below, and the bootstrapped compiler - and of nothing else, least of all which mod version is
being installed. So a built engine is kept in the Toolchain Cache under a name that is a hash
of exactly those inputs (`crates/toolchain/src/luajit/cache.rs`), and a later install that
would produce the same bytes uses it instead of spending the minute. Editing a patch or moving
the pin changes the name, so a stale engine cannot be handed back by accident; the engine is
still checked against the game's own imports every time, cache or no cache.

### The source is patched before it is built

The engine is a replacement for the exact Lua 5.1 the game ships, so where LuaJIT and PUC-Lua
5.1 disagree about behaviour the language leaves *undefined*, the mods were written against
PUC's answer and the engine has to give it. Those edits live in
`crates/toolchain/src/luajit/patches.rs`, one exact-text replacement each, applied to the
checkout before the first compile. A source that does not contain the expected text fails the
build rather than silently dropping the patch - which is the failure a maintainer wants when
the pinned commit moves.

**`table.insert(t, pos, v)` measures the table by its contiguous prefix.** `#t` on a table with
a hole is undefined - any index whose successor is nil is a valid answer - and PUC-Lua and
LuaJIT pick different ones, because their tables grow differently. `table.insert` uses that
number to decide how far to shift, so on a holey table the two engines build *different arrays*.
Vox Populi's top panel hits this: it inserts each strategic resource at its `StrategicPriority`,
the resources arrive in ID order (iron first) while the priorities put horses first, so the first
insert lands at position 2 of an empty table. PUC-Lua calls that table empty; LuaJIT calls it
length 2, shifts iron aside, and the next insert overwrites it - leaving a hole at index 2 that
stops the panel's `ipairs` after one icon. The patch measures the prefix instead, which is the
smallest valid border and equals `#t` exactly for any table without holes.

Measured against PUC-Lua 5.1.5 and stock LuaJIT across every shape of insert (append, at 1, in
the middle, at `#t+1`, past the end, into a hole), the only case whose resulting array differs
is the broken one, and there the patched engine matches PUC byte for byte.
