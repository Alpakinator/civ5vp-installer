# Replaced Files, and LuaJIT as the game's Lua engine

Civilization V loads its script engine from `lua51_Win32.dll` in the Game Installation root.
The shipped file is stock Lua 5.1.4 (PUC-Rio) - verified by its version string - and the game's
own executables, its stock DLLs and Vox Populi's `CvGameCore_Expansion2.dll` import 80 symbols
from it between them, every one of them a standard Lua 5.1 C API function. LuaJIT is
ABI-compatible with Lua 5.1 by design, so a 32-bit LuaJIT build renamed to `lua51_Win32.dll`
satisfies all of them. The community has been doing this since 2013.

Until now the installer has never written outside its Claimed Folders. That invariant is what
lets a player trust it beside their other mods, and it is stated in `CONTEXT.md` and
`docs/spec.md`. Replacing the engine cannot be done inside a Claimed Folder: the file belongs to
the game and lives in a directory the installer otherwise only reads.

## Decision

Introduce a third category beside Claimed Folders and Claimed Files: the **Replaced File**. A
Replaced File is a game-owned file the installer may overwrite, subject to four rules that
Claimed things do not need.

1. The original is copied into the App Data Store *before* the first replacement, and that backup
   is never written again. A second Deployment must not save LuaJIT over the stock engine.
2. Uninstall restores it. Removing the installer's work must leave the game with the engine it
   shipped with.
3. It is opt-in. The default configuration replaces nothing.
4. Opting back out restores it too. A Deployment that does not ask for the replacement puts the
   original back, so the choice is reversible by the same control that made it. Without this the
   checkbox is one-way, and the only route back is uninstalling everything else as well.

Rule 4 is driven by what the Backup Store holds, not by what the remembered settings say. An
older build that has never heard of this choice rewrites the settings file without it, so a held
backup is the only trustworthy evidence that an engine was replaced and still needs putting back.

The only Replaced File is `lua51_Win32.dll`.

LuaJIT is built from source with the bootstrapped toolchain rather than shipped as a prebuilt
DLL, for the reasons already given in ADR-0001: the installer does not deploy binaries it did not
compile. The pin is in `docs/pinned-artifacts.md`.

## Considered Options

- **Ship a prebuilt LuaJIT DLL** - rejected: it contradicts ADR-0001, and the binaries on offer
  are abandoned. The circulating community builds date from 2013-2017, and MoonJIT's repository
  has been archived since 2021.
- **Upgrade the game's SQLite as well** - not possible rather than rejected. SQLite 3.7.17 is
  statically linked into `CvGameDatabaseWin32Final Release.dll`, which exports 135 mangled C++
  symbols and no `sqlite3_*` entry points at all. There is nothing to bind a modern SQLite to.
- **A fourth injected boundary in the Core, for LuaJIT** - rejected: the work splits cleanly along
  the two that already exist. Fetching pinned source is the source provider's job and compiling it
  is the toolchain runner's, and neither needs to learn anything it does not already do.

## Consequences

- `GameFolders` gains the Game Installation root, which the Core was previously and deliberately
  not given. A path that decides where a game file gets overwritten is one detection must resolve,
  not one Sync may infer.
- Uninstall becomes stateful: it needs the backup store to do its job. A player who deletes the
  App Data Store between install and uninstall keeps LuaJIT, and the uninstall reports that rather
  than failing.
- Steam's "Verify integrity of game files" silently restores the stock DLL. The installer
  therefore treats a stock DLL in the game as "not deployed" and replaces it again on the next
  run, rather than assuming its own last write survived.
- The honest performance claim is narrow. The measured community results are for Lua-dominated
  work - map generation, the interface, script-heavy add-ons. Vox Populi's AI turn time is native
  C++ in `CvGameCore_Expansion2.dll`, which LuaJIT cannot affect, so the UI must not promise
  faster turns.
- Mods relying on Lua 5.1's deprecated implicit `arg` table in vararg functions break under
  LuaJIT, which never implemented it. That is the known source of breakage (InGame Editor,
  CivWillard, Cultural Capitals), and it is why the option is one a player turns on rather than
  one they inherit.
- **The engine is built from patched source.** A drop-in replacement has to agree with the
  engine it replaces about behaviour Lua leaves undefined, because that is what the mods were
  written against - not the standard. Vox Populi's own top panel proved it: `table.insert` with
  an explicit position measures the table with `#t`, `#t` on a table with a hole is undefined,
  and the two engines answer differently, which cost the panel every strategic resource icon but
  horses. Divergences of that kind are closed in `crates/toolchain/src/luajit/patches.rs` and
  documented in `docs/pinned-artifacts.md` §7. The rule for adding one: it must be behaviour the
  language leaves undefined *and* a case where the shipped engine's answer is the one mods
  depend on. Anything where LuaJIT is simply stricter than a mod expected - the `arg` table
  above - is the mod's bug and stays the mod's bug.
