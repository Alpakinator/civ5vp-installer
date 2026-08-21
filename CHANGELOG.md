# Changelog

Notable changes, newest first. Versions follow [semantic versioning](https://semver.org),
and each one is the tag the [releases page](https://github.com/Alpakinator/civ5vp-installer/releases)
publishes binaries for.

## Unreleased

## 0.1.4 - 2026-08-21

### Added

- **An `Open` button beside each folder box.** It opens that folder in your computer's own
  file manager, so you can drag files in, copy something out, or step down into the MODS
  folder - none of which the installer's own browser does, because it only picks a path. It is
  greyed out until the box names a folder that is really there. On Linux this saves retyping a
  path nine levels inside a Proton prefix.
- **A maintainer's `dll-flags.txt`, beside the installer executable.** It replaces the
  optimisation flags the DLL is compiled with, and nothing else, so a run measures the flags
  rather than a different build. Whitespace separates flags, `#` starts a comment, and a line
  reading `[linker]` sends what follows to the linker instead of the compiler. The installer
  says which flags it read before it builds, refuses to build on a flag the compiler would
  have ignored, and treats the flags as part of what identifies a build - so changing them
  rebuilds, and removing the file rebuilds back to the default. Not shown in the window: it is
  for measuring, and players get one default that has been measured for them.
- **A `Browse` button beside each of the three folder boxes.** If the installer did not find
  your game, or found the wrong copy of it, you can now click through to the folder instead of
  having to type or paste its path. The browser is drawn inside the installer window, so it
  works the same on every machine and needs nothing installed.
- **The browser opens near the answer rather than at the top of the disk.** It starts at
  whatever is in the box if that folder is there; failing that, at whatever the installer can
  detect, which it also fills in for you - so cancelling still leaves you better off than you
  started. For the Documents folder there is one more step: if nothing can be detected, it
  works out where the folder would be relative to the game folder you named and opens as deep
  into that as the folders actually go. On Linux that matters, because the Documents folder
  sits nine levels inside a Proton prefix.

### Changed

- **The DLL is compiled about 30% faster.** Turn times with many AI players are the part
  people feel, and that is what was measured - the same save, the same number of turns, one
  flag at a time, against the real game. The flags the build had inherited were chosen for
  size; the new ones are chosen for speed. Nothing about *why* the old ones were cautious has
  changed: the one setting that crashes the game is still switched off, and was confirmed to
  crash in three separate forms before anything else was touched.
- **The compiled DLL now needs SSE4.2, a 2008 instruction set.** It needed SSE3 (2004) before.
  Steam's July 2026 survey puts those at 97.88% and 98.02% of players, so this asks for
  fourteen-hundredths of a percentage point more than it already did, and the compiler can use
  twenty years of newer instructions in return. A wider target was tested and measured no
  faster, so it was not taken - it would have cost nearly three points for nothing. This only
  affects people who compile: installing a release deploys the DLL the release ships.
- **Multiplayer arithmetic is pinned down explicitly.** Letting the compiler use newer
  instructions also lets it fuse a multiply and an add into one instruction, which rounds
  differently. That is how multiplayer games drift apart and saves stop agreeing, so it is now
  switched off by name rather than by accident. If you play multiplayer, everyone should still
  be running the same DLL - which is what installing a release gives you.
- **The MODS, Text and DLC lines are shortened to one line each.** A Proton path is nine
  levels deep, so all three used to wrap, and the wrap landed in the middle of a folder name -
  three facts took six lines and read as damage. They now show the two ends that matter,
  `/home/you/…/Sid Meier's Civilization 5/MODS`, with the whole path on hover and in what a
  screen reader announces.
- **Radio buttons are diamonds.** They were the last round things left in a window where every
  other corner is square or cut at 45°.
- **Nothing in the window is rounded any more.** Buttons, the path boxes and the version box
  now have their corners cut at 45°, the same angle the panels around them have always had;
  checkboxes and everything else are square. The one part still drawn as plain rectangles is
  the file browser, which comes from a library and offers no way to cut them.
- **Installing an official release no longer builds anything, or downloads the build tools.**
  Vox Populi's releases carry the mod DLL they were built from - both the ordinary one and the
  43-civ one - so the installer now deploys that file instead of spending a first run
  downloading about 1.1 GB of Windows SDK and clang to reproduce it. A first install of a
  release is now a download of the mod files and nothing else. The build tools are fetched the
  first time something actually has to be compiled: an unofficial version, a branch or commit
  typed in by hand, your own checkout, or the LuaJIT engine.
- **Which releases get that is checked, not assumed.** Before deploying a checked-in DLL the
  installer asks the repository whether the version being installed is the one that last
  changed that file. That is true at a release and false one commit later, so an unofficial
  build a day after a release is still compiled - its DLL is older than the sources beside it -
  and a release tag typed into the custom-ref box gets the same treatment as picking it from
  the list. A release that ships no DLL for what you asked for is built rather than refused.
- **The unofficial-versions list now reaches back a release further.** It listed only the
  changes made *since* the newest release - so right after a release it was empty, and the
  changes that release had just shipped, which are exactly the ones people go looking for,
  were nowhere. It now starts at the release before last: `5.4.3.01…` are the changes that
  became `Release-5.4.4`, `5.4.4.01…` the ones since. Release commits themselves stay out of
  the list; they are offered as releases.
- **The version box never says "latest development version" any more.** It was not a version
  the picker offers - the unofficial-versions list names that same commit for what it is - but
  it could still end up *shown* as your selection, with no row in the list to change it back
  to. A settings file that still names it now opens on the newest release, which is also what
  a first run has always started from.
- **"Latest release - Release-5.4.4" now reads "Latest Release-5.4.4", and the newest release
  only ever goes by that name.** Picking it wrote its tag down, and the next run restored it as
  a plain `Release-5.4.4` - so the same install appeared under two names at once, one in the
  closed box and the other in the list below it. Whichever way you got there, the newest
  release now reads "Latest", and there is one row for it.
- **"Compile the DLL myself" is offered wherever a release would supply one.** It is off by
  default and costs the build-tools download, which the sentence by the Install button says
  again the moment you tick it.

## 0.1.3

### Changed

- **The first install downloads 102 MB of the Windows SDK instead of 1.4 GB.** The build needs
  eleven files out of that disc image - the rest is samples, documentation, debuggers and the
  64-bit halves, none of which is ever used. Each of the eleven is now fetched on its own,
  checked against its own checksum, so a first install pulls about 1.1 GB in total rather than
  2.4 GB, and the slowest part of it - the archive server that holds the SDK - is asked for a
  fourteenth of what it used to be.
- **The LuaJIT engine is built once, not once per install.** Nothing about it depends on which
  Vox Populi version you install, so installing a new one reused the engine already built
  instead of spending a minute rebuilding an identical file. It is rebuilt when something it is
  actually made of changes - the pinned LuaJIT source or the build tools.
- **Upgrading reclaims about 1.4 GB of disk.** If an earlier installer left the whole SDK
  image behind, this one takes the eleven files it needs out of it - off your own disk,
  downloading nothing - and then deletes the image. The remains of a failed image download go
  too: they were a resumable download of a file nothing asks for any more. This happens on the
  next install even if nothing needs building, and the Activity log says how much came back.

### Fixed

- **The LuaJIT engine no longer breaks the top panel's resource icons.** With the engine turned
  on, the strategic resources across the top of the screen showed horses and nothing else -
  iron, coal, oil, aluminium, uranium and paper all vanished. Vox Populi builds that row with
  `table.insert` at each resource's own priority, which depends on how the engine measures a
  table with a gap in it; Lua 5.1 and LuaJIT answer that differently, and LuaJIT's answer left a
  gap that stopped the row after the first icon. The engine is now built to answer the way the
  game's own Lua does, so the row is identical either way.
- **A dropped connection no longer ends the SDK download.** The Windows SDK image comes from
  the Wayback Machine, which drops ranged requests routinely; one drop used to abandon the
  whole 1.4 GB download and hand the player a failure. Each piece is now asked for again with
  a widening pause, the download as a whole is picked up twice more after that, and every
  request carries a deadline - before this, a wedged connection could hold one of the four
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

- CI runs the test suite and Clippy on Windows, not only on Linux. Windows-only code - game
  folder detection, the native LuaJIT host, the executable's resources - had never been
  executed by anything before.
- CI runs on every branch push and pull request, not only on release tags.

## 0.1.1

- The Vox Populi logo as the window and executable icon.

## 0.1.0

First release. Picks a version, downloads it, compiles the mod's DLL with its own
bootstrapped toolchain, and installs it as ordinary mods or as a baked modpack.
