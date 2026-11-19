# A listed-but-missing file is skipped, as the game skips it

A `.modinfo` can name a file the mod does not have: in `<Files>`, as an `<UpdateDatabase>` or
other action, or as an entry point. Two rules treated that as broken. Dev mode refused the
Deployment (`ModManifestMismatch`), and the Modpack refused to offer the mod ("its .modinfo lists
… which is not in the mod"). Both were wrong: the game runs such mods.

## The evidence

- **A player's report, 2026-11-18.** The Modpack picker refused four mods the player plays with.
  Three, as downloaded, list files they lack: Hokath's Proposals 32 files and 6 database
  actions, Enlightenment Era for VP 6 files and 1 action, Semper Fidelis two icon sizes. The
  fourth, Even More Resources, lacked a `Credits.txt` only in the player's copy.
- **Hokath's Proposals does it on purpose.** A feature is switched off by renaming its folder
  (`Pikeman_Rework` to `Pikeman_Rework_old`, others to `_WIP`) and leaving the modinfo alone, so
  its actions point at nothing. The mod is released and played that way.
- **The installer's own EUI Deployment.** EUI strips `LUA/` from `(1)` and `(2)`, whose modinfos
  still list 24 and 23 files there. On this machine's install the game's `Civ5ModsDatabase.db`
  has both mods Installed, Enabled and Activated: the game activated them without those files.
- **How the game reads a modinfo.** It stores every listed file in `ModFiles` (path, md5,
  `EvaluatedPath`, import) and resolves entry points when it uses them, through
  `Modding.GetEvaluatedFilePath` (`InGame.lua` line 1260). Nothing in that path requires the
  file to exist beforehand. It does not check the md5 either: a deployed DLL whose md5 differs
  from the modinfo's loads.

## The rule

A listed-but-missing file is a heads-up, never a refusal:

- **Dev mode** deploys, and the log names the files ("… lists N file(s) that are not in the
  folder, and the game will skip them: …"). Files present but unlisted were already only a
  heads-up.
- **Modpack**: such a mod is offered. The bake copies what the mod has, applies the database
  updates whose files exist, and skips a UI hook whose file is absent. Skipping is needed, not
  just harmless: in the pack every mod shares one virtual file system, so a hook by bare name
  could load another mod's file of that name. One log line per mod names what it lacks.
- What still refuses a mod is unchanged: an entry point the pack cannot hook at all (a game-setup
  `Custom` screen, a map script), an activation step a pack cannot run, and the
  selection-wide rules of ticket 16.

## What remains unverified

- **What the game does with a missing `<UpdateDatabase>`.** Not observed in a `Database.log`
  here. That it skips the action and goes on rests on the mods above working for their players,
  Hokath's six actions included.
- **A missing UI addin.** Assumed to fail to load and leave the rest running, as a missing
  context does elsewhere; the pack skips it outright, which comes to the same thing.
