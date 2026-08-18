# Changelog

Notable changes, newest first. Versions follow [semantic versioning](https://semver.org),
and each one is the tag the [releases page](https://github.com/Alpakinator/civ5vp-installer/releases)
publishes binaries for.

## 0.1.3 — unreleased

### Changed

- **The first install downloads 102 MB of the Windows SDK instead of 1.4 GB.** The build needs
  eleven files out of that disc image — the rest is samples, documentation, debuggers and the
  64-bit halves, none of which is ever used. Each of the eleven is now fetched on its own,
  checked against its own checksum, so a first install pulls about 1.1 GB in total rather than
  2.4 GB, and the slowest part of it — the archive server that holds the SDK — is asked for a
  fourteenth of what it used to be.
- **Upgrading reclaims about 1.4 GB of disk.** If an earlier installer left the whole SDK
  image behind, this one takes the eleven files it needs out of it — off your own disk,
  downloading nothing — and then deletes the image. The remains of a failed image download go
  too: they were a resumable download of a file nothing asks for any more. This happens on the
  next install even if nothing needs building, and the Activity log says how much came back.

### Fixed

- **The LuaJIT engine no longer breaks the top panel's resource icons.** With the engine turned
  on, the strategic resources across the top of the screen showed horses and nothing else —
  iron, coal, oil, aluminium, uranium and paper all vanished. Vox Populi builds that row with
  `table.insert` at each resource's own priority, which depends on how the engine measures a
  table with a gap in it; Lua 5.1 and LuaJIT answer that differently, and LuaJIT's answer left a
  gap that stopped the row after the first icon. The engine is now built to answer the way the
  game's own Lua does, so the row is identical either way.
- **A dropped connection no longer ends the SDK download.** The Windows SDK image comes from
  the Wayback Machine, which drops ranged requests routinely; one drop used to abandon the
  whole 1.4 GB download and hand the player a failure. Each piece is now asked for again with
  a widening pause, the download as a whole is picked up twice more after that, and every
  request carries a deadline — before this, a wedged connection could hold one of the four
  download threads open indefinitely. Pieces are 8 MB rather than 32 MB, so a failure costs
  seconds, and the progress lines name the transfer rate.
- **A failed install no longer signs off with "Finished".** The last line of the Activity log
  read "Finished in 4 min 50 s" whatever had happened, which read as success even though the
  failure was on screen above it. A run that did not finish now says it stopped.
- **A download that ends early is treated as interrupted, not as complete.** On the fallback
  single-connection path a truncated response was hashed as if it were the whole file, which
  discarded every byte that had arrived. It is now resumed from.

## 0.1.2

### Added

- **The LuaJIT engine, opt-in.** A new checkbox replaces the game's Lua engine
  (`lua51_Win32.dll`) with LuaJIT 2.1, built from source by the same toolchain that builds
  the mod's DLL. Map generation and interface scripts get faster; AI turn times do not
  change, because those are the mod's C++ code and no Lua engine touches them. Your original
  engine is saved before the first replacement and put back when you clear the box or
  uninstall. Some older Lua mods do not work under it. See
  [ADR-0006](docs/adr/0006-replaced-files-and-the-luajit-engine.md).

### Fixed

- **Clearing the LuaJIT checkbox now restores the game's own engine.** Previously the choice
  was one-way: turning it on replaced the engine, turning it off did nothing, and the only
  route back was uninstalling everything. The restore is driven by the saved original rather
  than by the remembered settings, so it works even if an older installer rewrote those.
- **The Activity log no longer collapses to a single line.** The whole page scrolls, so on a
  full window there was no height left by the time the log was drawn, and it shrank to one
  line exactly when an install had the most to report. It now always shows at least five
  lines and grows into whatever space is free.
- **Settings written by another version of the installer survive.** The settings file is
  rewritten whole on every save, so running an older build once silently discarded every
  choice only a newer build understood. Unrecognised keys are now carried across untouched.

### Changed

- CI runs the test suite and Clippy on Windows, not only on Linux. Windows-only code — game
  folder detection, the native LuaJIT host, the executable's resources — had never been
  executed by anything before.
- CI runs on every branch push and pull request, not only on release tags.

## 0.1.1

- The Vox Populi logo as the window and executable icon.

## 0.1.0

First release. Picks a version, downloads it, compiles the mod's DLL with its own
bootstrapped toolchain, and installs it as ordinary mods or as a baked modpack.
