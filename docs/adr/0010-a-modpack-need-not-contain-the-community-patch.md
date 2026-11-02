# A Modpack need not contain the Community Patch

Until ticket 15 every Flavor installed the Community Patch: "Vox Populi implies Community
Patch", and the code said "every Flavor includes the Community Patch" in four places - the
deployment matrix, the Modpack's DLL home, Sync's DLL step and the Build Fingerprint
sidecar. The player's own mods (ticket 12) could only ride on top of it.

The user asked for a Modpack of only their own mods, with neither Community Patch nor Vox
Populi. That is now a third Flavor, **Own Mods Only**.

## What it means

- **Nothing comes from the Installation Source.** No Version is fetched, the DLL is neither
  built nor taken as shipped, no Build Fingerprint is recorded. The source provider and the
  toolchain runner are not called at all, so such a pack needs no network, no Toolchain, and
  not even a named Version or checkout. LuaJIT is the exception: it is a file of the game,
  not of any mod, and is still built when asked for.
- **The pack runs the game's own DLL.** `Mods/` holds the picks alone; there is no
  `(1) Community Patch` inside it for a DLL to live in. The databases are merged onto the
  bare game, without Vox Populi's text.
- **Only as a Modpack.** In a Mods Deployment the player's mods are in MODS already, so the
  configuration would install nothing. `Plan::build` refuses it with a sentence, and the shell
  greys out whichever of the two choices would make the pair, asking the Core
  (`Flavor::is_legal_with`) rather than knowing.
- **At least one pick.** A pack of no mods would be the bare game baked into a DLC.
  Refused too.
- **43 Civs is ignored**, the way `extra_mods` is ignored in Mods mode: it changes which DLL
  goes into `(1)`, and there is no `(1)`. The shell greys the checkbox out
  (`Flavor::includes_community_patch`) but keeps its tick for when a Flavor with a DLL is
  picked again.
- Switching an existing Vox Populi Modpack to Own Mods Only replaces the pack wholesale, as
  any Modpack Deployment does, and removes `VPUI`, `UI_bc1` and the VPUI tips like any other
  Flavor that does not include them. MODS is still only read.

## Correction: a hook has to fit the file it goes into

The pack hooks a UI addin in by appending `g_uiAddins[#g_uiAddins + 1] = "stem";`. That only
does anything because the Community Patch's `InGame.lua`, `CityView.lua`, `LeaderHeadRoot.lua`
and `MiniMapPanel.lua` give `g_uiAddins` a `__newindex` that calls
`ContextPtr:LoadNewContext(value)`. The game's own copies have a plain table. Run with the line
appended, the real loaders made 0 `LoadNewContext` calls with the vanilla file and 1 with the
Community Patch's, so every UI addin in an Own Mods Only pack was silently dead.

So the hook now goes by the text of the file that ended up in the pack: with the `__newindex`
consumer it is today's line, byte for byte, so packs with the Community Patch do not change;
without it, `ContextPtr:LoadNewContext("stem");`. That also covers a mod whose override of a UI
file lacks the consumer. `MapGenerator.lua` is the exception. It has no UI context, and the
Community Patch's copy walks a plain list with `ipairs` later on, so a map-generation hook is
always a list line.

The same check decides what can provide a hook target: a `MiniMapPanel.lua` or
`MapGenerator.lua` counts only if it consumes hooks, not because the name matches. The offer
still assumes it of a Community Patch that has not been fetched; the build reads the real files.

**Not confirmed in the game.** The `LoadNewContext` line was checked against the loaders under
luajit with a stubbed `ContextPtr`, never in a running game. Whether a context loads correctly
from the end of a vanilla `InGame.lua`, `CityView.lua` or `LeaderHeadRoot.lua` needs an in-game
check. Also, the vanilla code keeps the returned context in `g_uiAddins`, where this line, like
the Community Patch's `__newindex`, discards it.

## What stays the same

The package is still `VP_MODPACK`, with the same manifest, GUID and Key as every Community
Patch modpack: the folder is a Claimed Folder under that name, and a second name would mean a
second Claimed Folder and a pack the in-game Modpack Maker's own cleanup does not know about.
Its in-game name therefore still reads "VP Modpack". Renaming it for this Flavor is possible
later, but the GUID/Key pair is checked by the game and is not ours to change.

The spec's "Managing any mods outside the Claimed Folders" stays out of scope. Baking a copy
of a mod into the Modpack reads it; the installer still writes nothing in MODS.
