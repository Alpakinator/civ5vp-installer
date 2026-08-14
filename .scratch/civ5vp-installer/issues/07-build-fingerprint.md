# 07 — Build Fingerprint + skip logic

**What to build:** Repeat installs are near-instant: the whole build is skipped when the Build Fingerprint (hash of all source inputs, compiler flags, 43-Civs setting, and Toolchain version) matches the sidecar recorded at last Deployment AND the deployed DLL's own hash still matches the recorded output. Any changed source, flag, or toolchain — or a manually swapped DLL — forces a rebuild. No false skips, no needless rebuilds.

**Blocked by:** 06 — Real DLL build.

**Status:** done

- [x] Unchanged inputs + intact deployed DLL → build skipped (verified through the Core seam)
- [x] Each of these alone forces a rebuild: edited source file, different configuration (Debug/Release, 43-Civs), different Toolchain version, tampered/replaced deployed DLL, missing sidecar — **Debug/Release is in the fingerprint but not yet reachable from a configuration; see Comments**
- [x] Fingerprint for a checked-out Version derives from the git tree; for a dirty Local Repo it derives from working-file contents
- [x] Skip decision is reported to the user ("DLL already up to date")

## Comments

Landed in the Core (`crates/core/src/fingerprint.rs` + the skip flow in `install.rs`), with
the source half supplied by the source-provider boundary: `SourceProvider::materialize` now
returns a `MaterializedSource { root, source_identity }` instead of a bare path. That is a
signature change to an existing boundary, not a third one (rule 2 intact).

### Measured, against the real toolchain (`real_install.rs`)

```text
first install:   60 s  (full 172-source compile + link + Sync)
repeat install:  155 ms — fetch, fingerprint check, skip, full Sync, cache clear
```

The skip is reported as "The DLL is already up to date — build skipped." and the repeat
install's Deployment is still exact (the DLL is re-copied from the verified deployed one, so
Sync's replace-wholesale semantics are unchanged).

### What the fingerprint is

One rendered block, compared byte-for-byte (rule 8), format-versioned (`fingerprint v1`) so a
future installer whose fingerprint means something different can never false-skip on an old
sidecar:

- **source identity** — from the boundary. Upstream Cache: `git:<commit>` (the acceptance
  criterion's "derives from the git tree": the commit names the tree, nothing is re-hashed).
  Local Repo: `files fnv1a64:<hash>` over `DLL_SOURCE_INPUT_ROOTS` — the seven compile roots
  plus `clang.cpp` — hashed from working-file contents, read-only (story 29's "as-is" holds).
  Deliberately the top-level roots rather than the per-Version file list: conservative, no
  false skips, while Lua/SQL/mod-content edits never force a needless compile.
- **version label** — it is compiled into the DLL as `commit_id.inc`.
- **Build Configuration** (Release/Debug) and **43-Civs** — with the toolchain identity these
  stand in for the compiler flags: every flag ticket 06 passes is a function of the four
  inputs above (the project file, which decides `STACKWALKER`, is part of the sources). If a
  flag ever stops being derivable from them it must join the fingerprint explicitly —
  documented on the type.
- **Toolchain identity** — already carries clang version, target, SDK, and layout version.

The sidecar (`CvGameCore_Expansion2.dll.fingerprint`, inside `(1) Community Patch`, so rule 6
holds) records the fingerprint plus the deployed DLL's own FNV-1a 64 hash; both must match.
Hashing is FNV-1a 64 implemented in the Core in a dozen lines — the Core keeps zero
dependencies (rule 1), and the job is integrity against accident (a swapped DLL, an edited
source), not an adversary. A sidecar write failure downgrades to "next install rebuilds",
never to a false skip, and does not fail an otherwise complete Deployment.

### Verified through the Core seam (`crates/core/tests/fingerprint.rs`)

With a counting fake runner (a skipped build is a build the boundary never saw) and an
editable copy of the fixture repository: unchanged → 1 build then skip, reported; Lua edit →
no compile but the file deploys; source edit → rebuild; 43-Civs toggle → rebuild; different
toolchain identity → rebuild; hand-tampered deployed DLL → rebuild and the marker is
restored; deleted sidecar → rebuild; deleted deployed DLL → rebuild. Plus a sources-crate
test that the Upstream Cache's identity is the checked-out commit, stable across repeats and
different across Versions.

### Post-review hardening

`/code-review` (both axes) drove five corrections after the ticket first closed:

- **The installer's own version joined the fingerprint.** The reviewers found the one real
  false-skip vector: the compiler flags are *derived by installer code* from the fingerprint's
  other lines, so a release that changes the derivation could have skipped against an old
  sidecar. The `installer <version>` line makes every release invalidate old sidecars — one
  honest rebuild per upgrade.
- **The Upstream Cache identity is now the commit's *tree* id** (`git-tree:<oid>`), so an
  amend or rebase to an identical tree no longer forces a needless rebuild — and it is the
  acceptance criterion's wording, taken literally.
- **A failed sidecar write is said out loud** (rule 11): a progress line names the path and
  says the next install will rebuild instead of skipping. Still never fails the Deployment —
  the failure mode stays "rebuild", never "false skip".
- **Path hashing uses OS bytes**, not a lossy string, so names differing only outside UTF-8
  cannot hash alike.
- The FNV step and the `release`/`debug` token each live in one place now.

Two reviewer observations recorded as accepted limitations: hand-editing files inside the
installer-managed Upstream Cache working tree is not detected (the cache is installer-owned
and "safe to delete, not to edit"; detecting it would mean re-hashing every install, undoing
the git-tree derivation's point), and the fingerprint's flag coverage remains by-derivation
plus installer version, not a literal flag-vector hash.

### The Debug/Release qualifier

`BuildConfiguration` is folded into the fingerprint and `BuildRequest`, but the Core still
hardcodes Release — no `InstallConfiguration` can say Debug until ticket 08's Dev mode adds
the choice. So "different configuration forces a rebuild" is proven for 43-Civs through the
seam, and for Debug/Release by construction only. Ticket 08 should add the seam test the
moment the toggle exists.
