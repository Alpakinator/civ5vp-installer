# Civ 5 VP Installer

One small program that installs [Vox Populi / Community Patch](https://github.com/LoneGazebo/Community-Patch-DLL)
for Sid Meier's Civilization V. Download it, run it, pick a version, click **Install** -
that's the whole process.

## What it does for you

- **Any version, one click.** The newest release is pre-picked; older releases are one
  dropdown away. An *unofficial versions* switch even lets you install any single change
  made since the newest release.
- **Builds the real thing.** Vox Populi's engine (a DLL) has to be compiled. The installer
  does that itself, on your machine - you don't install anything else, ever.
- **Modpack mode.** Instead of ordinary mods, it can bake everything into a *modpack* the
  game loads automatically - no Mods menu, works in multiplayer. Your own mods from the
  MODS folder can be baked in too, with a checkbox each.
- **Clean installs, clean exits.** Every install replaces the mod folders completely, so
  leftovers from an old version can never corrupt a new one. The **Uninstall** button puts
  your game back exactly the way it was.
- **Remembers everything.** Your folders, your choices, your own checkout path - the next
  run starts where you left off. Repeat installs of an unchanged version take under a
  second.
- **For modders.** *Dev mode* points at your own Community-Patch-DLL checkout and installs
  your working tree as-is - edit a file, click Install, see it in the game moments later.

## Install

1. Grab the file for your system from the
   [releases page](https://github.com/Alpakinator/civ5vp-installer/releases). One file,
   nothing else. What changed in each one is in the [changelog](CHANGELOG.md).
2. Run it. It finds your Civilization V folders by itself (you can correct it if it
   guesses wrong).
3. Click **Install**.

**Windows**: the Steam version of Civilization V.
**Linux**: the *Windows* version of the game running under Proton. (The native Linux port
can't load the mod's engine - the installer will tell you, not break your game.)

## The first run

The first install downloads about **2.4 GB** of build tools plus the mod itself, and
typically takes **10-25 minutes**. This happens once: everything is kept in the
installer's own data folder (**≈5 GB** when settled) and reused forever after. The
**Storage** panel shows where that folder is and can delete it in one click - your game
is never touched by that.

## Where things go

| What | Where |
| --- | --- |
| The mods | your game's `MODS`, `DLC` and `Text` folders - and nowhere else |
| Installer data (downloads, build tools, settings, log) | `%LOCALAPPDATA%\Civ 5 VP Installer` on Windows, `~/.local/share/civ5vp-installer` on Linux |

If anything goes wrong, the failure message has buttons that copy a ready-to-paste report
and open the log file.

## Building from source

```bash
cargo build --release -p civ5vp-installer
```

One self-contained binary per OS, built by CI on every version tag
(`.github/workflows/release.yml`). Developer documentation lives in
[`docs/spec.md`](docs/spec.md), [`CONTEXT.md`](CONTEXT.md) and
[`CODING_STANDARDS.md`](CODING_STANDARDS.md).

## License

[Apache-2.0](LICENSE). Provided as is, without warranty of any kind - see the license for
the full disclaimer. The installer only ever writes to the mod folders it manages and the
game's cache; **Uninstall** restores an unmodded game.

## Privacy

At launch the installer asks GitHub once whether a newer installer exists, and shows a
link if so. That is its only phone-home: no telemetry, no auto-update, nothing running in
the background. Offline it works normally and says nothing.
