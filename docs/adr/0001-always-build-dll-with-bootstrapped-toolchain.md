# Always build the DLL locally, with a first-run bootstrapped toolchain

The upstream repository's checked-in `CvGameCore_Expansion2.dll` is stale except at release commits, and users routinely install non-release versions of master — so the installer always compiles the DLL itself and never deploys a checked-in one, even for releases. The toolchain (pinned portable LLVM clang/lld + Windows SDK 7.0 / VC9 CRT extracted from Microsoft's ISO via the pinned archive.org URL) is downloaded and unpacked on first build into the app-data cache — never bundled in the executable (Microsoft redistribution license; ~700 MB) and never taken from the system (untested clang versions have produced DLLs Civ5 rejects).

## Considered Options

- **CI-built DLL artifacts per commit** — rejected: a local build takes 60–120 s while a GitHub Actions build takes ~10 min, and it makes every install depend on infrastructure someone must keep alive.
- **Trusting checked-in DLLs at release commits** — rejected to keep one code path and guarantee the DLL always matches the selected source.
- **Bundling the toolchain in the exe** — rejected: license and size.

## Consequences

First-ever build costs a ~700 MB one-time download (SDK ISO + LLVM); all later installs build in 1–2 minutes offline. Redundant rebuilds are avoided via the Build Fingerprint (see CONTEXT.md).
