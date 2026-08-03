# 02 — Full deployment matrix + Sync semantics

**What to build:** Every Install Configuration deploys exactly what the official installer would, and re-running or switching configurations always converges to a correct install. Covers all Flavors and toggles: EUI (LUA-strip from `(1)`/`(2)`, add `(3a)`, `UI_bc1` to DLC, legal only with Vox Populi), 43 Civs (43-civ DLL into `(1)`, slim `(3b)` with regenerated modinfo), Squads auto-included, `VPUI` DLC, tips XML to the Text Folder. Sync owns exactly the Claimed Folders and nothing else; strict fetch→build→Sync ordering means any failure leaves the existing install untouched.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

- [ ] Core-seam tests cover every legal Flavor/EUI/43-Civs combination and assert file placement matching the official installer's rules (the InnoSetup script is the behavioral reference) — **partial, see Comments**: all six combinations are covered with exact-tree assertions and no placement error was found in what is implemented, but three of the script's `[Files]`/`[InstallDelete]` groups are deliberately not implemented, so "matching the official installer's rules" is not yet true
- [x] Illegal combination (EUI with CP-only) is unrepresentable or rejected by the Core
- [x] Sync is exact and idempotent: stale files inside Claimed Folders deleted, Claimed Folders not in the configuration removed, second run is a no-op
- [x] Content outside the Claimed Folders is never touched (test with decoy mods/DLC present)
- [x] Game `cache` cleared after Deployment; `ModUserData` preserved
- [x] Uninstall removes all Claimed Folders and clears `cache`, restoring an unmodded fixture
- [x] Injected fetch/build failure aborts before any game folder is modified

## Comments

Implemented in the Core (`crates/core`), with 28 Core-seam tests: `tests/matrix.rs` (15) covers
the matrix, `tests/deployment.rs` (13) covers Sync semantics, cache clearing and Uninstall.

**The behavioural reference was read directly.** `VPSetupData.iss` on `LoneGazebo/Community-Patch-DLL@master`
is the official InnoSetup script (`scripts/release.py` compiles it with ISCC). It models six
mutually exclusive components; those six are exactly the six legal points of our two axes, and
`deployments_for` in `plan.rs` carries that mapping as a table. Findings worth recording:

- `VPUI` and `UI_bc1` are vendored directories at the **repository root**, not under a `DLC`
  directory and not separate downloads. `UI_bc1` is the EUI itself.
- The tips XML comes from `VPUI Text/VPUI_tips_en_us.xml` — a different folder from the `VPUI`
  deployed as DLC, and the folder name contains a space.
- The EUI Lua strip is top-level only. The official script excludes `\LUA` with a leading
  backslash, which anchors it to the source root; a nested `LUA` deeper in the tree survives.
- `Kit/` **is** installed by the official installer (it is in `(1)`'s `.civ5proj` and modinfo).
  The fork's `scripts/deploy_ingame.py` excludes it, but that script is a dev convenience, not
  the reference — the `.iss` wins, per `docs/spec.md`.

### Deliberately not implemented — needs a decision

**1. The base-game `Assets/DLC/Expansion2` replacement.** The official installer also writes
`Expansion2_VoxPopuli.Civ5Pkg` → `Assets/DLC/Expansion2/Expansion2.Civ5Pkg` and
`MinorCivSounds_*.xml` → `Assets/DLC/Expansion2/Sounds/XML/MinorCivSounds_VoxPopuli.xml`, so
that newly-added City-States play the right audio on selection. Both destinations are **outside
the Claimed Folders** as `CONTEXT.md` defines them, so doing it would breach coding-standards
rule 6. This is a genuine spec gap rather than an oversight: implementing it means extending the
Claimed set in `CONTEXT.md` (and shipping `Expansion2_Base.Civ5Pkg` so Uninstall can restore the
original). Left out pending that decision — the consequence is City-State selection audio.

**2. Legacy folder purge.** The official installer unconditionally deletes 19 historical mod
folder names (`(2) Community Balance Patch`, `(6a) …`, `(7b) …`, and so on) so that upgrades
from older VP versions do not leave a corrupt install. Same rule-6 problem: those names are not
Claimed Folders. Low risk to add, but it is still a change to the Claimed set.

**3. `(5) Modpack Maker for VP`** exists on `master` and is deployed by every component
(`.iss:53`). It is not in `CONTEXT.md`'s Claimed Folders list at all. There is a wrinkle beyond
the Claimed-set change: it is absent from the fork's `docker` branch and presumably from older
`Release-*` tags, so adding it naively would make installing an older Release fail with "the mod
files are missing the (5) Modpack Maker for VP folder". Deploying it needs a notion of a folder
that is optional by Version, which is a design decision rather than a line of code.

### Two smaller deviations, deliberate

- **Exclusions are a denylist, not the `.civ5proj` allowlist.** The official installer copies
  from a `Build/` staging tree containing only files listed in each mod's `.civ5proj`, so
  anything unlisted is invisible to it. We copy from the repository, so `tree.rs` names the
  documents and project files instead. Reading the allowlist is the faithful version and lands
  with ticket 06, which parses `.civ5proj` anyway for the DLL's source list.
- **`(3b)`'s modinfo is copied byte-for-byte, not regenerated.** The ticket's prose says
  "regenerated to match deployed contents", but the official installer does no such thing: it
  ships a modinfo listing a `CvGameCore_Expansion2.dll` that is not in the deployed folder. The
  acceptance criterion names the `.iss` as authoritative, and matching known-good upstream
  behaviour beat diverging from it. Note the same mismatch is unavoidable for `(1)` regardless,
  since we build our own DLL and its hash can never match the released one — so the game plainly
  does not verify these hashes.

### Also done here

`Plan::build` now refuses game folders whose MODS and Text do not share a parent. That is what
makes the `cache` folder a derived fact rather than a guess, which matters because clearing it
is the one write rule 6 permits outside a Claimed Folder.

### Not covered

The shell still hardcodes `Flavor::CommunityPatch`, so the matrix is not yet reachable from the
UI — `crates/installer/src/app.rs` is owned by ticket 03 this round. The Flavor/EUI/43-Civs
pickers are a follow-up once that lands.

## Review

Reviewed on both axes (`/code-review` against `5454f22`). Findings acted on:

**Rule 6 was widened silently — the most important finding.** The Claimed Files concept (for the
tips XML in the Text Folder) is a second exception to "nothing outside the Claimed Folders is
ever written", and shipping it while *declining* three other features on rule-6 grounds was
inconsistent. `CODING_STANDARDS.md` says to amend the rule rather than work around it, so:
`CONTEXT.md` now defines **Claimed Files** as a term, and rule 6 now names both sets, says both
are closed, and requires `CONTEXT.md` to be amended in the same commit as any addition. That
also settles the rule-16 finding that `ClaimedFile` was not a `CONTEXT.md` term.

**A user-facing message was wrong (rule 10).** A `(3b)` whose files had been renamed upstream
produced `The sources are missing the "*.modinfo, AdvancedSetup.lua" folder` — a glob, shown to
a non-programmer, described as a folder. `InstallError::MissingInSource` now carries a
`SourceItem` (Folder / File / Contents) and each reads as a sentence.

**Rule 7 hole.** The "this folder holds none of the files we take from it" check ran *during*
Sync, after removals had started. It now runs in `fetch`, beside the other source checks, so the
game is untouched. Covered by a new test asserting the game is byte-identical afterwards.

**A test blind spot.** The top-level anchoring of the EUI Lua strip was asserted only in
comments — the fixture had no nested `LUA`, so making the exclusion recursive passed everything.
The fixture now has `(1) Community Patch/Core Files/LUA/CoreHelper.lua`; making the exclusion
recursive now fails four tests.

Also: `copy_tree` and `copy_selected` were the same loop and are now one function;
`copy_file` no longer creates parent directories it never needed (it let any destination
materialise a directory tree); and `InstallOutcome`/`UninstallOutcome` now report Claimed Files,
which is what `ClaimedFile`'s public surface is for.

Not acted on, deliberately: the `"*.modinfo"` wildcard was flagged as Primitive Obsession, but a
type for one wildcard used in one place buys less than the comment explaining it costs.
