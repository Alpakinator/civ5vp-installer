# Changelog

Notable changes, newest first. Versions follow [semantic versioning](https://semver.org),
and each one is the tag the [releases page](https://github.com/Alpakinator/civ5vp-installer/releases)
publishes binaries for.

## 0.1.2 — unreleased

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
