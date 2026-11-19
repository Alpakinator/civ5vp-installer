# Modinfo Completion adds what ModBuddy would have

Upstream's mods are ModBuddy projects. The `.civ5proj` is the source; ModBuddy's Build, or
upstream's `scripts/generate_modinfo.py` (which `release.py` runs for every Release), writes the
`.modinfo` the game reads. Between Releases, commits change the `.civ5proj` and often forget the
committed `.modinfo`. A developer building with ModBuddy never notices, because their modinfo
is regenerated. A player installing that commit with this installer gets the stale one, and
this installer has no ModBuddy.

## What that has cost

- `62c77de6f` (#13491) added `VictoryTableChanges.sql` to `(1)`'s `.civ5proj` only. Every
  Modpack build of master failed with `no such column: PreferredGrandStrategy` until #13594
  added the two lines by hand.
- `Release-5.2` shipped `(4a)` without its `UI/Squads.xml` entry point (a generator bug, fixed
  upstream in `b17886d44`).
- Master since `dae4c612e` (#13489) lists `Gamecore/GamecoreInit.lua` and
  `GamecorePostProcess.lua` in `(1)`'s `.civ5proj` with `ImportIntoVFS=True`, and not in the
  modinfo. The DLL loads `GamecoreInit.lua` by bare name through the engine's `LoadFile`, which
  finds only files in the virtual file system. So a ModBuddy build probably runs that script and
  an installer build without completion probably does not. Not yet observed in a `Lua.log`; the
  script prints `GAMECOREINIT.LUA!` when it runs.

## The decision

Before Sync, each managed mod's `.modinfo` is completed from the `.civ5proj` beside it in the
source, and the completed copy is what is deployed and what the Modpack reads. **Add only, never
replace:**

- **Added**: an action, entry point or `<File>` the `.civ5proj` has and the modinfo lacks.
  Present means the same path with `\` and `/` alike and case ignored (an action is also matched
  on its set and type, an entry point on its type).
- **Never added**: a file that does not exist, or that this configuration does not deploy (EUI
  strips `LUA/` from `(1)` and `(2)`, and with 43 Civs `(3b)` gets only its modinfo and
  `AdvancedSetup.lua`). A line pointing at nothing is what ADR-0013 tolerates, not something to
  create.
- **Never touched**: properties, dependencies, references, blocks, and every committed line,
  including those the `.civ5proj` lacks. Some fixes land in the modinfo only: `185b99504` moved
  `CorporationChanges.sql` before `BuildingChanges.sql` there, and `251c3f842` listed six frame
  `.dds` files there first. Replacing the file would revert both.
- **Where**: right after the nearest entry the `.civ5proj` lists before it that the modinfo
  has, else first in its section; a missing section is created where the generator puts it.
  Action order is load order, so this matters for actions. For `<Files>` it only keeps the diff
  readable.
- **How it reads**: each added line is the generator's line, character for character: its
  indentation, its upper-case md5, `/` in file paths and the action's path as written,
  `&`, `<`, `>` and `"` escaped. The file keeps its BOM and its line ends. Nothing missing means
  the committed bytes, so a Release, whose modinfos `release.py` regenerated, deploys byte for
  byte.
- **A Local Repo is never written** (rule 6): only the deployed copy changes.
- **A `.civ5proj` that cannot be read** costs nothing: the committed modinfo deploys as before,
  and the log says why. A mod without one is left alone.

The log keeps one line per completed mod naming everything added, so a deployed modinfo that
differs from the committed one is never a mystery.

## MD5 in the Core

The generator writes an md5 for each file. The game does not check it on load (ADR-0013), but
an added line must match what ModBuddy writes, and the Core has no dependencies (rule 1). So the
Core carries RFC 1321 itself, tested against the RFC's suite and the padding edges. It is not a
security measure.

## Rejected

- **Regenerate the whole modinfo.** Reverts modinfo-only fixes, and a Release would no longer
  deploy as the official installer deploys it.
- **Run `generate_modinfo.py`.** A Python runtime is a dependency the installer may not have.
- **Only warn.** The player cannot act on it, and the Modpack would still fail to build.
