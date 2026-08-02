# Spec — Civ 5 VP Installer

**Status:** ready-for-agent
**Vocabulary:** all capitalized terms are defined in `CONTEXT.md`. Decisions marked ADR-N are recorded in `docs/adr/`.

## Problem Statement

Installing Community Patch / Vox Populi today is painful in every scenario except "Windows user installing the latest release":

- The official installer covers **releases only**. Anyone who wants a development version of upstream `master` — which players frequently do — must clone a multi-gigabyte repository, compile a C++ DLL requiring Visual Studio 2008-era tooling or a Docker-based toolchain, and hand-copy folders per scattered manual-install notes.
- **Linux players** (running the Windows game under Proton) have no installer at all.
- **Mod developers** iterating on the DLL, SQL, or Lua have no quick path from "edited a file in my checkout" to "the game is running my change" — today that is a hand-rolled script on one developer's branch, requiring Docker and Python.
- Manual installs go stale: leftover files from previous versions cause corrupt installs, and the community's standing advice is "delete things by hand and clear the cache".

## Solution

A single-file executable — **Civ 5 VP Installer** — for Windows and Linux, styled as if it were part of Civilization V itself (original art in the game's art-deco language, Tw Cen MT type). The user picks a Version (Release, latest development, or any ref — or their own Local Repo), a Flavor (Community Patch only / Vox Populi), and toggles (EUI, 43 Civs). The installer fetches sources incrementally so nothing is downloaded twice, **always compiles the DLL itself** with a bootstrapped, pinned clang toolchain (no Visual Studio, no Docker, no Python on the user's machine), and deploys via Sync into the game's MODS, DLC, and Text Folders — deterministically, with stale files removed and everything outside the Claimed Folders untouched. Failed downloads or builds leave the existing installation intact. Uninstall restores an unmodded game.

## User Stories

1. As a player, I want to install the latest Vox Populi release with a few clicks, so that I can play without reading manual-install notes.
2. As a player, I want to install the latest development version of `master`, so that I can play with fixes and features not yet in a release.
3. As a player, I want to update an existing install to a newer version, so that staying current is a one-button action rather than a reinstall.
4. As a player, I want updates to download only what changed since my last install, so that updating is fast on a slow connection.
5. As an advanced player, I want to install an arbitrary branch, tag, or commit, so that I can test a specific fix a developer points me to.
6. As a player, I want to choose between Community Patch only and full Vox Populi, so that I get the experience I prefer.
7. As a Vox Populi player, I want an EUI option, so that I get the enhanced interface installed correctly (including the compatibility file swaps) without following instructions.
8. As a Community-Patch-only player, I want the EUI option to be unavailable, so that I cannot produce a broken install.
9. As a player, I want a 43 Civs option with either Flavor, so that I can play larger maps, with the correct DLL built and placed automatically.
10. As a player, I want Squads included automatically with Vox Populi, so that I get the standard experience without extra choices.
11. As a player, I want the installer to find my MODS, DLC, and Text Folders by itself, so that I never have to know where they are.
12. As a player whose game is somewhere unusual, I want to pick the folders manually with validation, so that a wrong folder is caught before anything is written.
13. As a Linux player running the game under Proton, I want detection to find the folders inside my Proton prefix and Steam library, so that installation works the same as on Windows.
14. As a Linux player with the native Aspyr port, I want the installer to tell me plainly that VP requires the Windows version under Proton and refuse to deploy, so that I don't end up with a silently broken game.
15. As a first-time user, I want the toolchain to download and set itself up automatically with visible progress, so that I never install Visual Studio, Docker, Python, or any compiler myself.
16. As a returning user, I want every install after the first to work offline-fast from the caches, so that reinstalling or switching configurations takes about a minute.
17. As a player, I want the DLL build skipped when the deployed DLL already matches what would be built, so that repeat installs are near-instant.
18. As a player, I want a failed download or failed build to leave my current installation untouched, so that I can keep playing what I had.
19. As a non-programmer, I want build failures explained in plain language (with a suggestion to pick a Release), so that I'm not shown raw compiler output.
20. As a player, I want the full log saved to a file with copy/open buttons, so that I can report problems to developers usefully.
21. As a player, I want switching configurations (e.g. VP+EUI → CP-only) to remove exactly what no longer belongs, so that no stale files corrupt my game.
22. As a player with other mods installed, I want the installer to touch only its own Claimed Folders, so that my unrelated mods survive every operation.
23. As a player, I want the game's cache cleared automatically after each Deployment (with ModUserData preserved), so that I never hit the classic stale-cache corruption.
24. As a player, I want an Uninstall button, so that I can return to an unmodded game in one click.
25. As a player, I want to see where the installer stores its data and how large it is, and clear it with one button, so that I control the ~5 GB footprint.
26. As a player, I want my folder paths and last configuration remembered, so that updating later is a two-click affair.
27. As a player, I want to be notified when a newer installer exists, so that I can fetch it myself — without auto-update machinery running on my machine.
28. As a player, I want the installer to look and feel like Civilization V, so that it feels official and trustworthy rather than like a hacker tool.
29. As a mod developer, I want to point the installer at my Local Repo and have it build and deploy my working tree exactly as-is — uncommitted changes included, zero git operations performed on it — so that my edit-to-game loop is fast.
30. As a mod developer, I want the same Flavor/EUI/43-Civs options for my Local Repo as for GitHub versions, so that I can test any configuration of my changes.
31. As a mod developer, I want a Debug/Release choice in Dev mode, so that I can produce a debuggable DLL when I need one.
32. As a mod developer, I want to redeploy changed Lua/SQL while the game is running, so that I can hot-reload what the game permits; the installer must not block on a running game.
33. As a mod developer, I want the source file list read from the project file at the selected Version, so that builds don't break when upstream adds a source file.
34. As a Windows user on a locked-down machine, I want the installer to run as a lone exe without installation or admin rights, so that it works anywhere.

## Implementation Decisions

- **Application**: Rust + egui; one static binary per OS (Windows exe, Linux binary); no webview, no runtime dependencies beyond a desktop's standard graphics stack (ADR-0002). Name: "Civ 5 VP Installer". UI is English-only for v1.
- **Architecture / seams**: a **headless Core** is the single primary seam — it accepts an Install Configuration plus resolved folder paths, produces a plan, executes it, and reports progress/results. The egui layer is a thin shell over the Core. Inside the Core exactly two injected boundaries exist: the **source provider** (Upstream Cache / network) and the **toolchain runner** (compiler invocation). Everything else (detection, extraction, fingerprinting, Sync) lives behind the Core.
- **Sources**: two Installation Sources — the Upstream Cache (installer-managed, embedded-git incremental clone; blobless partial clone preferred, degrading to full clone if library support proves unreliable; nothing downloaded twice) and a Local Repo (used byte-for-byte as-is). Version tiers: Releases (`Release-*` tags), latest `master`, arbitrary ref.
- **DLL**: always built locally, never deployed from the repository (ADR-0001). clang-cl + lld with the settings proven in the docker-branch build (other clang configurations produce DLLs the game rejects). Toolchain Bootstrap on first build: pinned portable LLVM plus Windows SDK 7.0 / VC9 CRT obtained from the pinned archive.org ISO and extracted **in-process** (ISO9660 + MSI + CAB parsing inside the installer; no wine, msitools, or 7-Zip), with case-folding fixes applied on Linux. Build orchestration is ported into the installer — no Python. The source list comes from the `.civ5proj` project file at the selected Version. Compilation is incremental within a build; whole builds are skipped via the Build Fingerprint (input hash + deployed-DLL hash sidecar; both must match).
- **43 Civs**: DLL compiled with the 43-civ setting and placed in `(1)` as the only deployed DLL; `(3b)` receives only its `.modinfo` and `AdvancedSetup.lua`, with the modinfo regenerated to match deployed contents.
- **EUI**: legal only with Vox Populi. Deploys `UI_bc1` as DLC, swaps in `(3a)`, strips `LUA/` from `(1)` and `(2)`.
- **Deployment**: strict ordering fetch → build → Sync; failure at any stage aborts before the game is touched. Sync makes Claimed Folders exactly match the configuration (stale files deleted, unneeded Claimed Folders removed) and never touches anything else. Every Vox Populi configuration also deploys the tips XML to the Text Folder. Game `cache` cleared after each Deployment; `ModUserData` preserved. No running-game guard — deploying while the game runs is permitted deliberately. Uninstall removes all Claimed Folders and clears `cache`.
- **Detection**: Windows — known-folder API for Documents, Steam registry + `libraryfolders.vdf` for the game. Linux — Steam library parsing plus the Proton prefix for the Documents side; the native Linux port is detected only to be refused with an explanation. Manual picker with validation as universal fallback.
- **Storage**: everything (Upstream Cache, Toolchain Cache, settings, logs) in the platform app-data location; the executable is a lone file. UI shows store location/size and offers a clear-data button.
- **Art**: original artwork in Civ5's art-deco language, game assets used as visual reference only; Tw Cen MT embedded (ADR-0003).
- **Distribution**: own repository; CI-built binaries later; no auto-update — launch-time new-version notification only.

## Testing Decisions

- Tests exercise **external behavior only**, through the Core seam: given a fixture repository (a miniature Community-Patch-DLL layout) and temporary MODS/DLC/Text directories, run an Install Configuration and assert the resulting file tree. No test reaches into Core internals; the egui shell is not unit-tested.
- The two injected boundaries are faked: a fake source provider serving fixture trees, and a fake toolchain runner emitting a marker artifact as the "DLL". This keeps the suite free of the 580 MB SDK download and multi-minute compiles.
- Behaviors covered through this one seam: the full Flavor/EUI/43-Civs deployment matrix (mirroring the official installer's file placement), Sync exactness and idempotence, removal of stale Claimed Folders on configuration switch, non-Claimed content preservation, cache clearing with ModUserData preservation, fingerprint-based build skipping (including the tampered-DLL case), abort-before-touch on fetch/build failure, and Uninstall.
- Real-toolchain and real-clone **integration tests** exist but run manually / on-demand, not per-commit.
- Prior art: none — greenfield repository; these tests establish the house style.
- **Platform verification constraint**: development and routine verification happen on EndeavourOS (Arch) only. Windows and other-distro verification is currently unavailable locally; it arrives later via CI runners (and, imperfectly, wine smoke-tests). Windows-specific code paths (registry, known folders, extraction) must therefore stay behind the tested Core seam with platform adapters as thin as possible.

## Out of Scope

- macOS support (the Mac port has its own DLL incompatibilities).
- The native Aspyr Linux port as a deployment target (detected, refused, explained — never supported).
- Auto-updating the installer itself.
- CI-built DLL artifacts as an install path (rejected in ADR-0001).
- Managing any mods outside the Claimed Folders; general mod-manager features.
- Portable/exe-adjacent storage mode (explicitly reversed in favor of app-data).
- Installer UI localization.
- Windows/other-distro CI verification in v1 (constraint accepted; revisited when the repo gains CI).

## Further Notes

- Highest-risk technical bets, worth de-risking first: (1) blobless partial clone support in embeddable git libraries (fallbacks: full clone, or shipping a static git); (2) in-process ISO/MSI/CAB extraction fidelity against the real SDK ISO; (3) hand-crafted egui skin achieving genuine Civ5 fidelity.
- The official InnoSetup script and the docker-branch deploy script are the behavioral references for file placement; where they disagree with prose documentation, the InnoSetup script's placements win.
- The ~5 GB app-data footprint (git objects + working tree + toolchain) is a consequence of ADR-0001 and is surfaced honestly in the UI rather than hidden.
