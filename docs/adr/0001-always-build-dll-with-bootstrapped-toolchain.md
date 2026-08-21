# Always build the DLL locally, with a first-run bootstrapped toolchain

The upstream repository's checked-in `CvGameCore_Expansion2.dll` is stale except at release commits, and users routinely install non-release versions of master - so the installer always compiles the DLL itself and never deploys a checked-in one, even for releases. The toolchain (pinned portable LLVM clang/lld + Windows SDK 7.0 / VC9 CRT extracted from Microsoft's ISO via the pinned archive.org URL) is downloaded and unpacked on first build into the app-data cache - never bundled in the executable (Microsoft redistribution license; ~700 MB) and never taken from the system (untested clang versions have produced DLLs Civ5 rejects).

## Considered Options

- **CI-built DLL artifacts per commit** - rejected: a local build takes 60-120 s while a GitHub Actions build takes ~10 min, and it makes every install depend on infrastructure someone must keep alive.
- **Trusting checked-in DLLs at release commits** - rejected to keep one code path and guarantee the DLL always matches the selected source.
- **Bundling the toolchain in the exe** - rejected: license and size.

## Consequences

First-ever build costs a ~700 MB one-time download (SDK ISO + LLVM); all later installs build in 1-2 minutes offline. Redundant rebuilds are avoided via the Build Fingerprint (see CONTEXT.md).

## Correction (ticket 05)

This ADR said the SDK image would be read with an **ISO9660** parser and put first-bootstrap
traffic at **~700 MB**. Both are wrong, measured against the real artifact:

* the image is a **UDF** bridge disc - its ISO9660 side contains one `README.TXT` and none of
  the members the bootstrap needs, so an ISO9660-only reader cannot extract the toolchain at
  all;
* the image alone is **1.45 GiB**, and with the portable LLVM a first bootstrap moves about
  **2.4 GB**.

Neither changes the decision - the DLL is still always built locally from a bootstrapped,
pinned toolchain, and extraction still happens in-process with no external programs. What
changes is the cost the UI has to be honest about, and one line of the implementation contract.
`docs/pinned-artifacts.md` carries the evidence and a two-command way to re-check it.

## Correction (Shipped DLLs at Release commits)

This ADR rejected "trusting checked-in DLLs at release commits", to keep one code path and
guarantee the DLL always matches the selected source. The first half of that reasoning still
holds; the second turns out to be wrong about Releases, and the evidence is in the repository:

```bash
# For Release-5.4.4, Release-5.4.3, Release-5.4.2 alike, the tag's own commit is the last
# one that touched both checked-in DLLs, and its message is "X.Y.Z Release".
curl -s "https://api.github.com/repos/LoneGazebo/Community-Patch-DLL/commits\
?sha=Release-5.4.4&path=%281%29%20Community%20Patch/CvGameCore_Expansion2.dll&per_page=1"
```

Upstream refreshes `(1) Community Patch/CvGameCore_Expansion2.dll` and
`(3b) 43 Civs Community Patch/CvGameCore_Expansion2.dll` **in the Release commit itself**. At
that commit - and one commit later, no longer - the checked-in DLL was built from the sources
beside it. So the guarantee this ADR wanted is available there without compiling anything.

What changes:

* A **Release** deploys the DLL it ships. Nothing is compiled, and the Toolchain Bootstrap's
  2.4 GB is never fetched - which for most players is now the whole install.
* **Every other Version still compiles**: unofficial builds, `master`, any branch or commit
  typed in, and every Local Repo. Their checked-in DLL is older than the sources beside it,
  exactly as this ADR said.
* Which case applies is **checked, not assumed**: the source-provider boundary asks GitHub
  which commit last changed that path at the commit being installed and compares it against
  the commit being installed. A tag whose DLL was not refreshed answers no and is compiled,
  and so is an Arbitrary Ref that names a Release commit - correctly, because it *is* one.
* When that lookup cannot be made - a dropped connection, or GitHub's unauthenticated hourly
  limit reached from a shared address - a weaker check stands in: does a `Release-*` tag point
  at this commit, asked over the same `ls-refs` the version picker already uses (no API, no
  rate limit, no objects transferred). It proves the commit is a Release rather than proving
  the DLL was refreshed there. That is a deliberate trade: the two have never disagreed, and
  the alternative is handing a player who picked a Release the gigabyte of build tools this
  whole path exists to spare them. If even `ls-refs` fails, the API failure is reported and
  the DLL is compiled.
* A Release that ships no DLL for the configuration (an old tag, or `(3b)`'s copy missing) is
  compiled rather than refused, with a progress line saying so.
* The player can still ask for a compile: "Compile the DLL myself" is offered wherever the
  Shipped DLL could apply, and rides in the Build Fingerprint so ticking it really rebuilds.

The rejected option that stays rejected is CI-built artifacts. The one code path this ADR
wanted is preserved where it matters: the DLL is produced or accepted in one place, verified
against the commit it belongs to, and deployed by Sync like any other file.
