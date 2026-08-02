# 02 — Full deployment matrix + Sync semantics

**What to build:** Every Install Configuration deploys exactly what the official installer would, and re-running or switching configurations always converges to a correct install. Covers all Flavors and toggles: EUI (LUA-strip from `(1)`/`(2)`, add `(3a)`, `UI_bc1` to DLC, legal only with Vox Populi), 43 Civs (43-civ DLL into `(1)`, slim `(3b)` with regenerated modinfo), Squads auto-included, `VPUI` DLC, tips XML to the Text Folder. Sync owns exactly the Claimed Folders and nothing else; strict fetch→build→Sync ordering means any failure leaves the existing install untouched.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

- [ ] Core-seam tests cover every legal Flavor/EUI/43-Civs combination and assert file placement matching the official installer's rules (the InnoSetup script is the behavioral reference)
- [ ] Illegal combination (EUI with CP-only) is unrepresentable or rejected by the Core
- [ ] Sync is exact and idempotent: stale files inside Claimed Folders deleted, Claimed Folders not in the configuration removed, second run is a no-op
- [ ] Content outside the Claimed Folders is never touched (test with decoy mods/DLC present)
- [ ] Game `cache` cleared after Deployment; `ModUserData` preserved
- [ ] Uninstall removes all Claimed Folders and clears `cache`, restoring an unmodded fixture
- [ ] Injected fetch/build failure aborts before any game folder is modified
