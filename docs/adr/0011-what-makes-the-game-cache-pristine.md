# What makes the game's cache pristine

A Modpack build starts from `cache/Civ5DebugDatabase.db`, which has to be the vanilla merge. It
is kept once as `modpack-base` and reused forever, so a modded cache taken for pristine poisons
every later pack. `cache_state` looked only for `CustomModOptions`, the Community Patch's own
table, so a database changed by any other mod read as pristine. Applying a real mod twice gave
no error, just different output (+2233 bytes).

## The evidence

The game itself cannot be run for this, but 34 real cache folders were on disk: trashed copies
from two months of the user's sessions, plus one from a headless harness. Each holds
`Civ5ModsDatabase.db` beside the `Civ5DebugDatabase.db` of the same session, opened read-only.
Truth was taken from content: every table compared with the `modpack-base` snapshot, leaving out
the three `MapScript*` tables, which only record the map files the game scanned.

- **6 caches are vanilla.** All 6 have `Activated = 0` in every row, with 3 to 6 rows `Enabled`.
  So `Enabled` is only the tick in the Mods menu, and says nothing.
- **27 caches are modded with `Activated > 0`.**
- **1 cache is modded with `Activated = 0`**: the headless harness, where no mod is even
  `Enabled`, so its mods came some other way than the Mods menu. `Activated` records the Mods
  menu, not the file.
- **One of the modded caches was caught mid-change**: 4 of 5 rows back at `Activated = 0`, and
  the mod database written 49 seconds after a gameplay database that was still modded. So the
  column can run ahead of the file it would have to speak for.
- In all 6 vanilla caches the gameplay database was written after the mod database, by 3 to 11
  seconds.

## The rule

A cache is pristine only if all of these hold, which classified all 34 correctly:

1. no `CustomModOptions` table;
2. no row in `Mods` with `Activated` set;
3. the mod database was not written after the gameplay database;
4. it is the full merge, as before: `Civilizations`, populated.

No usable mod database means nothing vouches for the file, so it is handled like an unreadable
cache. Every real cache has one, and the game writes it on launch.

The Core adds a fifth condition the assembler cannot see: **no Modpack may be deployed** while a
snapshot is taken. Sync clears the cache, so a cache existing beside a deployed pack was written
with that pack loaded, and a pack leaves no mark in rules 1 to 3 when it has no Community Patch
in it. That is the headless case, in the form a player can reach. Launching the game cannot fix
it, so the sentence says to uninstall first.

## What remains unverified

- **Every modded sample includes the Community Patch.** No cache here comes from a session with
  only other mods activated. `Mods.Activated` is the engine's record rather than the Community
  Patch's, so nothing suggests it would differ, but that case was not observed.
- **Backing out of the Mods menu.** Whether the game rewrites the gameplay database when mods are
  deactivated was not observed. Rule 3 is there for it: if the file is left modded, the mod
  database is the newer one.
- **A launch after a modded session, in the same folder.** Every sample is one folder at one
  moment. The game must clear `Activated` by the next main menu, since mods do not stay active
  across launches, and then rewrite the gameplay database; if it did not, this rule would refuse
  until the cache is cleared, which is a nuisance and not a poisoned pack.
- **Rule 3 assumes file times survive.** A cache copied by hand could be refused. One launch fixes
  that.

An existing `modpack-base` is not re-checked: it has no mod database beside it to ask. The one on
the development machine was compared table by table and is clean; one taken by an older build
from a cache modded without the Community Patch stays in use until it is deleted.
