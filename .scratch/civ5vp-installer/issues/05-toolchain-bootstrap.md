# 05 — Toolchain Bootstrap: downloads + in-process SDK extraction

**What to build:** The first time a build is needed, the installer downloads the pinned portable LLVM and the Windows SDK 7.0 ISO from its pinned source with visible progress, extracts the SDK in-process (ISO9660 + MSI + CAB parsing — no wine, msitools, or 7-Zip on the user's machine), applies case-folding fixes on Linux, and caches everything in the Toolchain Cache. Every later build starts instantly from the cache. This resolves the spec's extraction-fidelity bet.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

- [ ] Pinned LLVM and SDK ISO downloads are checksum-verified, resumable or cleanly restartable, and reported with progress
- [ ] In-process extraction of the real SDK ISO produces a layout equivalent to the docker image's known-good extraction (verified by file inventory/hash comparison)
- [ ] Case-folding fixes applied so the headers/libs resolve on a case-sensitive filesystem
- [ ] Bootstrap runs once; subsequent builds detect the populated Toolchain Cache and skip it
- [ ] Interrupted bootstrap leaves a state that self-repairs on retry
- [ ] Slow integration test (real downloads) exists but is excluded from the per-commit suite
