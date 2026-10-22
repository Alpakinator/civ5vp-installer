# A Modpack's map scripts go to the Documents-side `Maps` folder, their scan-time text to `Text`

In a Modpack game, game setup listed none of the map scripts that only Vox Populi or a baked-in
mod brings: VP's `Communitu_79a` and `Continental_Drift`, or Tectonic. The scripts were inside
`VP_MODPACK` all along.

## Why the game did not find them

The game fills its map list three ways (`stopwatch.log` names the passes):

- **Base Game Map Scripts**: `Assets/Maps`, plus the `MapDirectory` of every *registered*
  content package. `VP_MODPACK` is not registered DLC. Its GUID is absent from the game's
  `DownloadableContent` table, so a `<MapDirectory>` in `MPModsPack.Civ5Pkg` is ignored. Tried
  in-game under both `<Gameplay>` and `<GameplaySkin>`, with the cache cleared.
- **Modder Map Scripts**: the `MapScript` entry points of *activated* mods. A Modpack activates
  none.
- **The Documents-side `Maps` folder**, subfolders included. Confirmed in-game: scripts placed
  in `Maps/VP_MODPACK` were listed and generated maps.

VP's scripts named like one of the game's own (`Amazon_XP`, `Arborea`, `Great_Plains_XP`, …)
were listed all along, as "… for VP". The game finds the base file and the VFS hands it the
pack's copy, which wins by bare name. Only the uniquely named ones were missing.

## Decision

A Modpack Deployment copies each staged mod's `MapScript` entry file into
`Documents/…/Maps/VP_MODPACK`, a new Claimed Folder (`ModpackMaps`). It leaves out:

- scripts named like a map file of the game's own (`Assets/Maps`, each DLC package's `Maps`),
  which already reach game setup through the base file, and a copy would list twice;
- scripts named like one already in the player's own `Maps`, which stay the one listed.

Some scripts build their name at scan time: `Name = L("TXT_KEY_TECTONIC") .. " - v6"`. The
scan runs before the pack's text is loaded, so those names showed as raw keys. The text the
game *does* have at scan time includes the Documents-side `Text` folder, so the Deployment
also writes `Text/VP_MODPACK_MapScripts.xml` (Claimed File `ModpackMapScriptText`). It holds a
`Replace` for every `TXT_KEY_…` literal in the copied scripts that the merge *added*: present
in the merged localization database, absent from the pristine base. Every language that has
the string is included. The rule needs no knowledge of how a script calls `L()`, and it never
writes over a string the game already has.

`MapScript` entry points no longer make a mod unbakeable: a mod that carries its map scripts
bakes in whole.

## Consequences

- A Mods Deployment and Uninstall remove both, like the pack itself. Both are left out
  entirely when the pack has no script to list, rather than deployed empty.
- A mod whose map script depends on the Mods menu is still refused, by its other entry
  points. Giant Earth's setup screen is a `Custom` entry point, and its script needs
  `Modding.GetActivatedModVersion`, which a Modpack game cannot answer.
- The Modpack boundary grew one field (`text_tags`), and `merge_and_dump` now returns the
  added strings, computed while the merged database is still open.
- Only a Modpack Deployment writes these. The pack is a file a player can hand to others,
  but the map scripts and their text are outside it: a player given a bare `VP_MODPACK`
  folder gets the "… for VP" scripts and not the unique ones, exactly as before.
