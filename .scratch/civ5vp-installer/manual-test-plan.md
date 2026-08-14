# Manual test plan — everything, before the first GitHub release

The human pass over the whole installer against the real game, in one sitting. Order
matters: the cheap and reversible things first, the store-clearing test last (it deletes the
4-plus-gigabyte caches). Everything here was machine-verified already — this pass is for the
things only a person at the real game can judge.

**The binary:** `target/release/civ5vp-installer` (or `cargo run --release`).

## 0. One-time setup

```bash
# Reuse the already-populated toolchain so the first build starts instantly instead of
# downloading 2.4 GB from archive.org:
mkdir -p ~/.local/share/civ5vp-installer
ln -sn ~/.cache/civ5vp-toolchain ~/.local/share/civ5vp-installer/toolchain-cache
```

Know before you start:

- Installing **will modify your real game's** MODS/DLC/Text folders — that is the test. Any
  existing manual Vox Populi install in there gets replaced by Sync (by design). Saves and
  `ModUserData` are never touched; the Uninstall button restores an unmodded game.
- The first GitHub install fetches ~600 MB of sources. Repeats are incremental.
- The log is at `~/.local/share/civ5vp-installer/installer.log` if anything looks wrong.

## 1. Launch, detection, window

- [ ] Both game folders pre-filled correctly (Proton install found)
- [ ] Break a folder path by hand → plain-language refusal with Copy details / Open log; fix
      it → refusal disappears
- [ ] Window: resize up from the 900×780 minimum — nothing clips or breaks
- [ ] The art-deco skin reads right to your eye on your real screen (ticket 09's
      ready-for-human check — fonts, colors, spacing at your DPI)

## 2. Version picker

- [ ] The version list arrives (~1 s), combo reads "Latest release — Release-5.4.3"
- [ ] The combo lists many releases, newest first, plus "Latest development version" and
      "Custom branch, tag, or commit"
- [ ] Pick an older release, relaunch the app → the pick was remembered
- [ ] Offline check: pull the network, relaunch → lookup fails into a sentence with
      Copy/Open buttons and a working "Try again" (after reconnecting)

## 3. First real install (the player path)

Suggested: **Latest release + Vox Populi with EUI** (the default).

- [ ] Progress narrates fetch → build → sync; "Compiling N of 172" during the build
- [ ] Finishes with the installed-folders summary (expect (1), (2), (3a), (4a), VPUI,
      UI_bc1 + the tips XML)
- [ ] **In the game**: Mods menu shows the folders; start a game; the version string on the
      main menu / diplomacy corner reads the Release tag with "Installer"
- [ ] Play a few turns — the DLL is genuinely running your build

## 4. Repeat + skip (user story 17)

- [ ] Click Install again, same configuration → "The DLL is already up to date — build
      skipped.", done in about a second

## 5. Configuration switches

- [ ] Switch to **Community Patch only** → (2), (3a), (4a), VPUI, UI_bc1 disappear from the
      game, (1) stays (Sync exactness)
- [ ] Toggle **43 Civs** on with a Flavor → rebuild happens, `(3b) 43 Civs Community Patch`
      appears with just its two files; in-game advanced setup offers 43 civs
- [ ] Switch back to full VP+EUI → everything returns

## 6. Tamper resistance

- [ ] Overwrite the deployed `CvGameCore_Expansion2.dll` with any other file → Install →
      it rebuilds/redeploys instead of skipping

## 7. Dev mode

- [ ] Pick "My own checkout", point at a Community-Patch-DLL clone (a junk folder is
      refused with a sentence)
- [ ] The **Debug build** checkbox appears (and only here)
- [ ] Edit a Lua file in the checkout → Install → done in well under a second, edit visible
      in `MODS/…`; with the game open on a Lua screen, the hot-reload loop works
- [ ] Debug install completes (bigger DLL; game still loads it)

## 8. Uninstall (user story 24)

- [ ] Click Uninstall → all Claimed Folders and the tips XML gone, game launches unmodded,
      saves and ModUserData intact

## 9. Storage panel — run this LAST

- [ ] Open Storage → real location, size ≈ 5 GB, Recalculate works
- [ ] **Clear stored data** → folder empty; the app keeps running
- [ ] One more install from nothing: it re-fetches sources and — since the symlink is gone —
      does the real 2.4 GB toolchain bootstrap. Only run this if you want to sit through the
      archive.org download once as the true fresh-player experience; otherwise re-make the
      symlink from step 0 first.

## 10. Expected silences

- The update notification stays absent: the `Alpakinator/civ5vp-installer` repo does not
  exist yet, and the check is silent by design (the log records the failed ping).

## When everything above is ticked

1. Create the GitHub repository `Alpakinator/civ5vp-installer` (or rename it in
   `crates/installer/src/update.rs` and `README.md` first), push this repo to it.
2. `git tag v0.1.0 && git push --tags` — CI builds both binaries and attaches them to the
   GitHub Release.
3. Download the released Linux binary and spot-check it launches; the Windows exe awaits a
   Windows machine.
