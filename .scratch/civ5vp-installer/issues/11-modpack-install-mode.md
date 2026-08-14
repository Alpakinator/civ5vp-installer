# Ticket 11: Modpack install mode

Status: in progress

## What the user asked for

> add ability to create and insert a VP modpack to civ 5 instead of installing the mod.
> (5) Modpack Maker for VP does this, but it's not that convenient. in this case don't
> install the mod within MODS, do only modpack. but if vp is in MODS, and user wants a
> modpack, don't delete the MODS. but when user wants a mod and modpack is installed,
> it's important to delete VP modpack, cause it will conflict with mod when user
> activates the mod inside the game.

## What a modpack is

`Assets/DLC/VP_MODPACK/` inside the game folder, loaded automatically at startup like
official DLC — no Mods menu, works in multiplayer. The reference implementation is the
Community Patch DLL's own MPMP functions (`CvGame::CreateMPMP` and friends,
CvGame.cpp ~14520-14940) driven in-game by `(5) Modpack Maker for VP`'s Lua:

- `MPModsPack.Civ5Pkg` — constant manifest (GUID `{b5932ae4-0f4f-498f-9333-e2d31b20e095}`,
  Priority 300, UISkin Expansion2Primary with GameplaySkin dirs `Mods` and `UI`).
- `Mods/<mod name>/` — verbatim copy of each activated mod's folder (including the
  Community Patch DLL binary; the game's VFS finds it there). The Modpack Maker mod
  itself (ID `eb8f6ed3-109d-4f2f-a81d-516c8d2f91c1`) is excluded.
- `UI/` — `InGame.lua`, `CityView.lua`, `LeaderHeadRoot.lua` copied from
  `Assets/DLC/Expansion2/UI/...`; any mod file named `InGame.lua`, `CityView.lua`,
  `LeaderHeadRoot.lua`, `MiniMapPanel.lua`, `MapGenerator.lua` is copied over them; for
  every activated entry point of type InGameUIAddin / CityViewUIAddin /
  DiplomacyUIAddin / MiniMapOverlayAddin / PreMapGenScript a line
  `g_uiAddins[#g_uiAddins + 1] = "<file stem>";` is appended to the matching UI file.
- `Override/` — one empty file for every `*.xml` under the game's `Assets/` whose first
  50 lines contain `<GameData>` (so base XML stops double-loading), plus two dumps:
  - `CIV5Units.xml` — the FULL merged gameplay database as one GameData XML: tables in
    `sqlite_master` name order; per table a `<Table>` schema block from
    `PRAGMA table_info` (skipped for engine-created schemas: ArtDefine_*, Audio_*,
    Map_Folders, Map_Sizes, Maps — exact list in ModpackMaker.lua), then `<Delete/>`,
    then every row as `<Row>` with non-empty, non-nil columns XML-escaped. Engine
    tables skipped entirely: ApplicationInfo, DownloadableContent, MapScript*, Maps?,
    ScannedFiles (exact list in ModpackMaker.lua SKIPPED_TABLES).
  - `CIV5Units_Mongol.xml` — the localization DB for the 10 VP languages as
    `<Language_xx><Replace Tag=...><Text>/<Gender>/<Plurality>` entries.

## How the installer builds one offline (no game launch, the whole point)

The in-game tool exists only because it needs the *merged* database. Offline we can
reproduce that merge:

- **Base gameplay DB**: `<documents>/cache/Civ5DebugDatabase.db` after a vanilla launch
  is the complete base+expansions+DLC merge (verified on the reference machine: 45
  Civilizations, no `CustomModOptions` table). Preflight guards pristine-ness: if
  `CustomModOptions` exists (a Community Patch table) the cache is contaminated by a
  modded session — tell the user to start the game to the main menu once, quit, retry.
- **Base text DB**: `<documents>/cache/Localization-Merged.db`, same launch.
- **Mod updates**: parse each deployed mod's `.modinfo` (skip the Modpack Maker by ID);
  execute its `OnModActivated > UpdateDatabase` files in listed order, mods in
  load-order (folder-name order matches VP's numbering): `.sql` via execute_batch,
  `.xml` via the documented GameData semantics (`<Table name=>` DDL, `<Row>`,
  `<Update><Where/><Set/>`, `<Replace>`, `<Delete>`, boolean true/false -> 1/0 by
  column type; `Language_*` tables route to the text DB).
- Dump both DBs in the exact MPMPM format above.
- Assemble the pack in the App Data Store, deploy the freshly built DLL into the
  staged `Mods/(1) Community Patch/`, then Sync the whole tree to the Claimed Folder
  `Assets/DLC/VP_MODPACK` in the game folder.

**Verification gift**: after the user activates VP mods in-game once, the game rewrites
`Civ5DebugDatabase.db` as the exact merged DB our engine must produce — diff
table-by-table.

## Architecture (rule 2 amendment)

The Core stays zero-dep, so SQLite cannot live there. Third injected boundary:
`ModpackAssembler` in `core::boundaries` — `merge_and_dump(job)` where the job carries
base DB paths, the ordered update list, and the two dump output paths; plus a
`base_state()` pristine check. Implemented by new crate `crates/modpack`
(rusqlite bundled + quick-xml; rule 17 justification: SQLite is the game's own storage
format, no pure-Rust reader is trustworthy enough to write game data; quick-xml is pure
Rust). Everything else — modinfo parsing (small std XML scan like toolchain's vcxproj
parser), pack assembly, Civ5Pkg, UI wiring, override emptying, Sync — is concrete Core.

## Deletion semantics (the user's exact rules)

- Modpack install: write only `Assets/DLC/VP_MODPACK`; MODS untouched even if VP is there.
- Mod install: deploy MODS as today AND delete `VP_MODPACK` if present (conflict).
- Uninstall: removes both, like any Claimed Folder.

## UI

Install-mode choice (persisted): "Classic install — mods activated in-game" vs
"Modpack — baked into the game as DLC, loads automatically, multiplayer-friendly".
Failure notice for contaminated cache with the support buttons. Summary sentences name
what was written and what was removed.

## Progress

- [x] Rule 2 amendment + `ModpackAssembler` boundary + job types in core
- [x] Modinfo parsing + load order + update collection in core (`core/src/modpack.rs`)
- [ ] crates/modpack: SQL apply, GameData XML apply, boolean/typing semantics (subagent)
- [ ] crates/modpack: gameplay dump + text dump in MPMPM format (golden tests) (subagent)
- [x] Core assembly: Civ5Pkg, Mods copy, UI addin wiring, Override emptying
- [x] Preflight pristine check + snapshot-into-store + error surface (5 seam tests green)
- [x] Deletion semantics + uninstall coverage (seam tests green)
- [x] Installer UI mode picker + persistence + shell test (snapshots pending a compile)
- [ ] End-to-end on this machine incl. in-game verification diff
