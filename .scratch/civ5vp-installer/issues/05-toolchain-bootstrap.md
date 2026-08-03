# 05 — Toolchain Bootstrap: downloads + in-process SDK extraction

**What to build:** The first time a build is needed, the installer downloads the pinned portable LLVM and the Windows SDK 7.0 ISO from its pinned source with visible progress, extracts the SDK in-process (ISO9660 + MSI + CAB parsing — no wine, msitools, or 7-Zip on the user's machine), applies case-folding fixes on Linux, and caches everything in the Toolchain Cache. Every later build starts instantly from the cache. This resolves the spec's extraction-fidelity bet.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

**Every URL, checksum, ISO member and fix-up this ticket needs is recorded in `docs/pinned-artifacts.md`.** Read it first. Two things there override older prose: the VC9 CRT ships **inside the same Windows SDK ISO** (`Setup/vc_stdx86/`), so there is exactly one mandatory download, not two; and the proven compiler is **clang 18** targeting `i386-pc-windows-msvc`. Do not take build settings from upstream `master` — its Release configuration produces a DLL the game rejects.

- [ ] SDK ISO download verifies against the recorded SHA-256 before extraction, is resumable or cleanly restartable, and reports progress
- [ ] Portable LLVM 18.x is fetched and pinned; its version is part of the Toolchain identity and feeds the Build Fingerprint
- [ ] In-process extraction pulls exactly the ISO members listed in `docs/pinned-artifacts.md`, honouring each MSI's CAB-name-to-real-path mapping rather than guessing
- [ ] All six Linux fix-ups from `docs/pinned-artifacts.md` are applied (lowercase + backward symlinks, case-mismatched `#include` resolution, `Include`/`Lib` symlinks, backslash-to-slash in `#include` directives, per-`.lib` case symlinks, WDK header stubs)
- [ ] Extraction is verified against the docker image's known-good result: `windows.h`, `stdio.h`, `iostream`, `kernel32.lib`, `msvcrt.lib` and `DriverSpecs.h` all resolve, and header/lib counts match the committed reference baseline
- [ ] Case-folding fixes applied so the headers/libs resolve on a case-sensitive filesystem
- [ ] Bootstrap runs once; subsequent builds detect the populated Toolchain Cache and skip it
- [ ] Interrupted bootstrap leaves a state that self-repairs on retry
- [ ] Slow integration test (real downloads) exists but is excluded from the per-commit suite
