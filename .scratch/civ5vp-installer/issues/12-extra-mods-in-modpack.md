# Ticket 12: Bake the player's own mods into the Modpack

Status: done

## What the user asked for

> how could we implement making a modpack with some mods that the user has in MODS?
> perhaps an additional box for selecting extra mods THAT THE USER HAS IN MODS?
> (that aren't CBP or vp)

## What was built

- `available_extra_mods(mods_folder)` in the Core: every MODS folder holding a
  `.modinfo`, minus the Claimed Folders (the managed set) and the in-game Modpack Maker
  (excluded by its mod ID `eb8f6ed3-…`, the same way it excludes itself). Sorted.
- `InstallConfiguration.extra_mods: Vec<String>` — folder names, meaningful in Modpack
  mode only, persisted in the settings file as a `|`-joined line (`|` is illegal in
  Windows folder names, so it cannot collide).
- Assembly stages the picks after the managed set — a modmod's database changes must
  land on top of Vox Populi's — with the standard source exclusions applied; their
  modinfo entry points and special UI files go through the same wiring; their
  UpdateDatabase files are appended to the merge job in the same order.
- MODS is only read; the copies live inside `VP_MODPACK`. A pick that has vanished
  fails the Deployment before the game is touched; a pick naming a managed folder is
  skipped with a progress line.
- UI: when "Install as a modpack" is selected and the player has own mods, a checkbox
  list appears ("Also bake in your own mods from the MODS folder"). Ticks are
  remembered and re-intersected with what actually exists at every folder resolve.

## Coverage

Core-seam: offer lists only real own-mods; picked mod baked after the managed set with
update order asserted; vanished pick refuses pre-touch. Settings round-trip with picks.
Shell: tick + install + baked-file assertion + remembered tick across relaunch.

## Notes / deferred

- Load order among multiple extra mods is alphabetical (their numbering conventions
  usually encode intent); no drag-to-reorder UI unless someone needs it.
- Mods with their own DLL are effectively neutered inside a pack (the Built DLL is the
  one the VFS finds first) — same as the in-game tool; not detected specially.
