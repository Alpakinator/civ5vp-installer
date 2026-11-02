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
- **The pack runs the game's own DLL, unless a ticked mod brings one** (see the correction
  below). `Mods/` holds the picks alone; there is no `(1) Community Patch` inside it for a
  Built DLL to live in. The databases are merged onto the bare game, without Vox Populi's text.
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
- **The "Install from" panel is greyed out** while nothing comes from it, in place and with its
  values kept. The player sees what will not be used, and nothing moves.
- **The Vox Populi intro is treated as cleared.** The shell greys the checkbox out and keeps its
  tick; `InstallConfiguration::plays_the_vox_populi_intro` is what the Core goes by, so a saved
  configuration asking for the intro deploys nothing, and an intro swapped in earlier goes back
  exactly as it does when the box is cleared. Silencing the menu theme stays available: that
  option is not Vox Populi's.
- Switching an existing Vox Populi Modpack to Own Mods Only replaces the pack wholesale, as
  any Modpack Deployment does, and removes `VPUI`, `UI_bc1` and the VPUI tips like any other
  Flavor that does not include them. MODS is still only read.

## Correction: a mod may bring the game DLL when nothing else does

The first version refused every mod carrying a `CvGameCore_Expansion2.dll`, on the reasoning
that a Modpack's DLL is the Built DLL or the game's own. That is only true beside the Community
Patch. A pack of Own Mods Only has the place free, and a DLL mod for the unpatched game is
exactly what a player would want such a pack for.

- **The game loads one DLL, so this is a cross-mod rule**, a clash in `settle_selection` beside
  `<Blocks>`, live in the picker and again in the build. A mod with a DLL is refused in any
  Flavor with the Community Patch. Without it, the first to bring one holds the place: the
  others are refused by that mod's name and come back when it is unticked. Two ticked anyway,
  from settings, keep the first in selection order.
- **A mod with several copies is refused on its own**, in every Flavor: which one to run is not
  a choice the installer can make for it.
- **Staging puts the DLL back.** `tree::copy_selected` strips the file at every depth, which is
  right for the managed mods (ADR-0001's correction), so `assemble` copies a kept mod's DLL
  across afterwards, at its path inside the mod. The Sync of the stage is verbatim, and the pack
  goes wholesale on Uninstall or on the next Modpack, so the DLL goes with it.
- **It is the mod's file, not a Built DLL.** `built_dll` stays `None`, no fingerprint sidecar is
  written, and the DLL reuse check, which looks in `(1) Community Patch`, does not see it.

**Not seen in a running game.** Nobody has tested whether the game loads a DLL from inside a
pack for a mod that is not the Community Patch. The tests show the file lands where the mod
keeps it, not that the game runs it. Until that is checked in game, a player's pack with such a
mod may run the game's own DLL under a database made for another.

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
