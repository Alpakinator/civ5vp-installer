# Always build the DLL locally, with a first-run bootstrapped toolchain

The upstream repository's checked-in `CvGameCore_Expansion2.dll` is stale except at release commits, and users routinely install non-release versions of master — so the installer always compiles the DLL itself and never deploys a checked-in one, even for releases. The toolchain (pinned portable LLVM clang/lld + Windows SDK 7.0 / VC9 CRT extracted from Microsoft's ISO via the pinned archive.org URL) is downloaded and unpacked on first build into the app-data cache — never bundled in the executable (Microsoft redistribution license; ~700 MB) and never taken from the system (untested clang versions have produced DLLs Civ5 rejects).

## Considered Options

- **CI-built DLL artifacts per commit** — rejected: a local build takes 60–120 s while a GitHub Actions build takes ~10 min, and it makes every install depend on infrastructure someone must keep alive.
- **Trusting checked-in DLLs at release commits** — rejected to keep one code path and guarantee the DLL always matches the selected source.
- **Bundling the toolchain in the exe** — rejected: license and size.

## Consequences

First-ever build costs a ~700 MB one-time download (SDK ISO + LLVM); all later installs build in 1–2 minutes offline. Redundant rebuilds are avoided via the Build Fingerprint (see CONTEXT.md).

## Correction (ticket 05)

This ADR said the SDK image would be read with an **ISO9660** parser and put first-bootstrap
traffic at **~700 MB**. Both are wrong, measured against the real artifact:

* the image is a **UDF** bridge disc — its ISO9660 side contains one `README.TXT` and none of
  the members the bootstrap needs, so an ISO9660-only reader cannot extract the toolchain at
  all;
* the image alone is **1.45 GiB**, and with the portable LLVM a first bootstrap moves about
  **2.4 GB**.

Neither changes the decision — the DLL is still always built locally from a bootstrapped,
pinned toolchain, and extraction still happens in-process with no external programs. What
changes is the cost the UI has to be honest about, and one line of the implementation contract.
`docs/pinned-artifacts.md` carries the evidence and a two-command way to re-check it.
