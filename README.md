# Civ 5 VP Installer

One file that installs [Community Patch / Vox Populi](https://github.com/LoneGazebo/Community-Patch-DLL)
for Sid Meier's Civilization V — any released version or the latest development build —
styled like it belongs to the game.

- **Pick a version**: the newest release (default), any older release, the latest
  development version of `master`, or any branch/tag/commit.
- **Always a real build**: the installer compiles the mod's `CvGameCore_Expansion2.dll`
  itself with its own downloaded clang toolchain. No Visual Studio, no Docker, no Python —
  nothing to install besides the installer.
- **Exact installs**: deployments replace the mod folders wholesale, so stale files from an
  older version can never corrupt a newer one. The game's cache is cleared automatically;
  your `ModUserData` is never touched.
- **Fast repeats**: sources are fetched incrementally (nothing is downloaded twice), and
  when nothing changed, the build is skipped outright — a repeat install takes well under a
  second.
- **Dev mode**: point it at your own Community-Patch-DLL checkout and it builds and deploys
  your working tree exactly as-is, uncommitted changes included, with a Debug/Release
  choice. Edit a Lua file, click Install, and the change is in the game in about a tenth of
  a second.

## Install

1. Download the binary for your OS from the
   [releases page](https://github.com/Alpakinator/civ5vp-installer/releases) — one file,
   nothing else.
2. Run it. It finds your Civilization V install and Documents folder by itself (both are
   editable if it guesses wrong).
3. Pick a version and a flavor, click **Install**.

**Windows**: the Steam install of Civilization V.
**Linux**: the *Windows* version of Civilization V running under Proton. The native Aspyr
port cannot load the mod's DLL — the installer detects it and explains rather than
producing a broken install.

## What to expect on the first run

The first install does real work, and the honest numbers are:

- **~600 MB** download of the mod's sources (incremental afterwards — switching versions
  later transfers only what changed).
- **~2.4 GB** one-time download of the build tools: a pinned portable clang and the Windows
  SDK the mod's DLL must be built against, unpacked in-place by the installer itself. The
  archive.org half of that download is slow — budget an hour or more.
- **~1 minute** of compiling (172 C++ files) on a typical machine.

Altogether the installer's data folder settles at **≈5 GB**. Every later install reuses all
of it: a repeat install of an unchanged version takes ~0.1 s, and a version switch costs one
incremental fetch plus one compile. The **Storage** panel in the installer shows exactly
where the folder is and how large it is, and its **Clear stored data** button deletes it all
(never touching the game) — the next install simply re-downloads.

## Where things live

| What | Where |
| --- | --- |
| Installer data (sources, build tools, settings, log) | `%LOCALAPPDATA%\Civ 5 VP Installer` on Windows, `~/.local/share/civ5vp-installer` on Linux |
| Installed mods | your game's `MODS`, `DLC` and `Text` folders |
| Log file | `installer.log` in the data folder — the failure panel has copy/open buttons |

The installer only ever writes to the mod folders it owns and the game's cache; everything
else in your game is left alone. Uninstalling the mods means deleting the `(1)`–`(4a)` mod
folders, `VPUI` and `UI_bc1` — the installer's own data folder can go whenever you like.

## Building from source

```bash
cargo build --release -p civ5vp-installer
```

One self-contained, single-file binary per OS, built by CI from a clean checkout and
attached to the GitHub Release on every version tag (`.github/workflows/release.yml`).
Development docs live in [`AGENTS.md`](AGENTS.md) and [`docs/spec.md`](docs/spec.md).

## Version notification

At launch the installer asks GitHub (once, in the background) whether a newer installer
release exists and shows a link if so. There is no auto-update and nothing runs in the
background afterwards; offline, the installer works normally and says nothing.
