# Modpack audit following #13364

## Scope and evidence

The missing VPUI text exposed an input-completeness problem: a successful merge
does not establish that every input the game normally loads was supplied. The
audit therefore checked source selection, manifest actions, UI entry-point
wiring, XML/SQL application, localization export, base-cache reuse, and the
fetch/build/Sync ordering. It is a targeted Modpack audit, not a claim that every
installer or gameplay behavior has been verified.

References inspected:

- Upstream `Release-5.4.6` (`1a8e6ecacb9a7c37695c2403e8f844ba007038e2`):
  `(5) Modpack Maker for VP/ModpackMaker.lua`, the MPMP functions in `CvGame.cpp`,
  and the managed mods' manifests.
- The user's `5.4.6_VP-EUI_SSEM.zip`: all six mod manifests, action-file presence,
  UI addin targets, special UI filenames, and game DLL placement.
- The installer Core seam with fixture repositories and both fake boundaries and
  the real SQLite assembler. Tests do not deploy into the user's actual game.

## Reproduced and fixed

Each new regression failed before its corresponding fix.

| Problem | Result before the fix | Correction |
| --- | --- | --- |
| Manifest text was scanned without excluding comments or lifecycle sections | Disabled SQL and `OnModDeactivated` cleanup actions entered the merge; commented UI hooks could be activated | Ignore comments and read database actions only inside `Actions/OnModActivated`; scope UI hooks and file lists to their manifest sections |
| UI overrides were matched without case, but copied under their original spelling | On Linux, `ingame.lua` survived beside `InGame.lua`; hooks were added to the base copy instead of the override | Copy special UI files to the canonical filename used by the hooks |
| Localization export compared schema names case-sensitively | A translation successfully applied to `language_fr_FR` disappeared from the final dump | Use SQLite's `NOCASE` comparison, matching the game's identifier lookup |
| Extra-mod selections were used as paths and were not checked for duplicates | A caller could escape staging with a path, or include the same mod's database updates twice | Reject path syntax, invalid filename characters, and duplicate names during planning |
| XML detection sliced the last four bytes of arbitrary UTF-8 filenames | A non-XML filename such as `图图` caused a panic | Check the path extension without slicing UTF-8 |

The Core regression tests cover deployed file contents and the database job's
input list. The localization regression uses the real SQLite assembler and
checks the exported text, rather than only checking the intermediate database.

Validation: **436 tests passed, 24 ignored**, `cargo clippy --all-targets -- -D
warnings` passed, and `cargo build --release` passed. The resulting
`target/release/civ5vp-installer --help` runs. This was local Linux verification;
the ignored real-clone, Toolchain Bootstrap, and game-DLL compilation tests were
not run for this audit.

## Findings for the reported pack

- The six manifests declare **424 database actions**. All corresponding files
  are present except the already-known upstream dangling reference to
  `Database Changes/Text/en_US/CoreGameOptionTextChanges.xml`. Upstream renamed
  that file to `.sql` without updating the action; the installer logs and skips
  it, matching the game's behavior. This audit does not silently substitute a
  different upstream action.
- All **18 UI addin targets** are present.
- There is one game DLL, in `(1) Community Patch`, and exactly one copy of each
  special UI entry file inside the Modpack.
- The manifests in this particular pack do not contain the commented-action
  case above. The real English language view also uses the expected spelling.
  These new findings establish additional failure cases, not additional proven
  symptoms in that download.

## Remaining limits

- **Extra-mod dependency order is not resolved.** Ticket 12 deliberately uses
  the selected folder order (alphabetical in the UI). An extra mod that depends
  on another extra mod may therefore run too early. The installer is not yet a
  general mod dependency resolver; the standard VP order and the two extras in
  the reported pack do not reveal this problem.
- **The saved base is not tied to a game installation, DLC set, or language.**
  `ensure_base_snapshot` reuses an existing pair without a configuration identity.
  Changing those inputs can leave it stale. Likewise, the override scan sees the
  current Assets tree while the base may predate a newly installed DLC. A robust
  solution needs base provenance and refresh rules, not another list of text
  filenames. This remains an architectural follow-up.
- **Pristine-cache detection is a VP-specific heuristic.** Absence of
  `CustomModOptions` does not prove that no unrelated mod ever modified a cache.
- **Runtime database processing is not simulated.** The DLL performs ID remapping
  and other database post-processing when the game loads. The upstream Maker
  runs inside that environment; the offline tests do not exercise the game engine.
- **No independent in-game output comparison was available at audit time.** A
  full comparison needs a pack generated in-game from the same release, DLC set,
  language, and extra mods. Tests and code review cannot establish equivalent
  gameplay by themselves.

The audit changes are local development changes; they are not part of the
already-published 0.1.6 release.
