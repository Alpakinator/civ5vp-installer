# 10 — Release readiness

**What to build:** Someone who has never seen this conversation downloads one file and successfully installs Vox Populi. Storage panel shows the App Data Store location and size with a working clear-data button (never touching the game); logs are saved with copy/open buttons wherever errors surface; launch pings for a newer installer release and shows a notification link (no auto-update); CI builds the two single-file binaries (Windows exe, Linux binary); README covers download, first run, and the ~5 GB footprint honestly.

**Blocked by:** 02, 03, 07, 08, 09.

**Status:** done — with the publication steps below still needing a human (creating the GitHub repository and pushing the first tag)

- [x] Storage panel reports real location/size; clear-data empties the App Data Store and the next install re-bootstraps cleanly
- [x] All failure surfaces route through the plain-language panel with log save/copy/open
- [x] New-version notification appears when a newer release exists; absent otherwise; launch works offline
- [x] CI produces both binaries from a clean checkout; Linux binary verified locally, Windows binary built by CI (verification deferred per the spec's platform constraint) — **the workflow is written and cannot run until the repository exists; see Comments**
- [x] README with install/usage instructions and footprint disclosure
- [x] Fresh-machine walkthrough (clean App Data Store, empty MODS) succeeds start to finish

## Comments

The ticket implied one more thing than its checklist named: with no Version picker in the
UI, "someone downloads one file and installs Vox Populi" was impossible — the shell only
offered the Local Repo path. So the biggest piece here is the **player path**:

### The Version picker

`SourceProvider` gained `available_versions` (an extension of an existing boundary, not a
third one — rule 2), and `VersionCatalog` moved from `civ5vp-sources` into the Core so the
picker is fakeable offline (rule 13; the fast suite and the screen previews draw it from a
fixture catalog, never a socket). The shell offers **Download from GitHub** — newest Release
pre-picked, every Release listed, the Latest Development Version, and a custom-ref escape
hatch — or **Dev mode's checkout field**; the pick is remembered like everything else. The
Install button stays disabled while "Latest release" does not yet know which release that is
— it must never quietly install something other than what it says (the shell rule is
drawn-or-disabled; what a Version *means* stays in the Core/sources).

### Measured

```text
fresh-machine walkthrough:  128 s — clean App Data Store, empty game folders, real GitHub:
                            listed versions, picked Release-5.4.3 (the actual newest),
                            fetched it, compiled its DLL, deployed Vox Populi + EUI complete
                            (crates/installer/tests/real_install.rs::a_fresh_machine_…)
```

The walkthrough's one concession: `CIV5VP_TOOLCHAIN_CACHE`, when set, is symlinked in as the
Toolchain Cache so each run does not re-download 2.4 GB from archive.org — that path has its
own end-to-end proof (ticket 05's `real_bootstrap.rs`). Everything else, including the
upstream fetch, starts from nothing. Bonus evidence: this built the DLL of the *newest*
upstream Release, not just ticket 06's cached checkout — the Version-tracking build
(project-file parsing, STACKWALKER, the CRT stubs) holds on live upstream.

### The support surfaces

- **Log file**: `log_detail` appends to `installer.log` in the App Data Store (and echoes to
  stderr). Every failure notice — install failures, the game-folder refusal, the
  version-lookup failure — carries **Copy details** (the sentence + the log tail) and
  **Open log**. The platform opener is rule 5's second documented exception, amended into
  CODING_STANDARDS.md in the same change.
- **Storage panel**: location, measured size (computed when the panel opens, not per frame),
  **Clear stored data** (disabled mid-install), and the promise that clearing never touches
  the game — `AppDataStore::size_on_disk`/`clear`, seam-tested (clear leaves an empty store
  the next install re-bootstraps from) and shell-tested (the game files survive the click).
- **Update ping** (user story 27): one background GET at launch; the tag is scanned out of
  GitHub's JSON without a parsing dependency and compared numerically; every failure is
  silence in the UI and a line in the log. Offline launch never waits on it. Wired only in
  the real binary — tests and previews have no channel to receive from.

### What still needs a human before the first release

1. **Create the repository** the constants and links name: `Alpakinator/civ5vp-installer`
   (`crates/installer/src/update.rs`, README). It does not exist yet; until it does, the
   update ping finds nothing (silently, by design) and the README's download link is dead.
   Renaming instead means changing both constants and the README together.
2. **Push a `v*` tag.** `.github/workflows/release.yml` then builds both binaries from a
   clean checkout, runs the fast suite + clippy on Linux, and attaches
   `civ5vp-installer-linux-x86_64` / `civ5vp-installer-windows-x86_64.exe` to the GitHub
   Release. The Windows binary is built-not-verified, exactly as the spec's platform
   constraint allows; first manual Windows run is the remaining verification.
3. The Linux CI binary is glibc-dynamic from `ubuntu-latest` (one self-contained file, not
   static in the musl sense — README says so honestly). If old-distro reports appear, a musl
   or older-baseline build is the follow-up.

### Post-review notes

`/code-review` (two axes) found no hard violations. Its real findings are folded in above:
the two failure surfaces that lacked copy/open buttons got them, the CI workflow gained the
actual create-Release step, the README's "static binary" wording was corrected, update-check
failures now reach the log, and the fixture catalog lives in one place
(`placeholder::fixture_version_catalog`). Accepted judgement calls, recorded here: the
"wait for the catalog" gate and the newest-release fallback live in the shell (drawn-or-
disabled presentation of a Core-typed choice); `update.rs` keeps its logic and unit tests in
the installer crate — installer self-update is not game domain and has no place in the Core.

### The window grew

900×640 → 900×780: the picker and storage panels are real height. All `egui_kittest`
baselines re-rendered and reviewed at the new size (rule 15); the size is also the window
minimum, and `--screenshot` follows `cli::DEFAULT_SIZE`.
