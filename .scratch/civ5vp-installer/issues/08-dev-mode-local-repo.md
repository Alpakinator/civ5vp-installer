# 08 — Dev mode: Local Repo

**What to build:** A mod developer points the installer at their own Community-Patch-DLL checkout (folder picker, remembered, validated) and it builds and deploys the working tree exactly as-is — uncommitted changes included, zero git operations performed on it. Same Flavor/EUI/43-Civs options as the GitHub path, plus the Debug/Release choice exposed only here. Deploying while the game is running is permitted (the Lua/SQL hot-reload loop).

**Blocked by:** 02 — Deployment matrix, 06 — Real DLL build.

**Status:** done

- [x] Local Repo selection validates the folder is a Community-Patch-DLL checkout and is remembered across runs
- [x] Deployment content comes byte-for-byte from the working tree, uncommitted changes included; no git command ever runs against the Local Repo
- [x] All Install Configurations work from a Local Repo; Debug/Release visible only in Dev mode
- [x] Fingerprint-based skip works for the dirty working tree (via ticket 07 semantics)
- [x] No running-game guard: deployment proceeds with the game open
- [x] End-to-end demo: edit a Lua file in the checkout, redeploy, changed file is in MODS

## Comments

Most of Dev mode already existed by the time this ticket started — the Local Repo provider
(ticket 04's `local.rs`, byte-for-byte, no git), the real build from any tree (ticket 06),
and content-derived fingerprints for dirty trees (ticket 07). What this ticket added:

- **Checkout validation** (`crates/sources/src/local.rs`): a named folder must contain
  `CvGameCoreDLL_Expansion2/` — the one directory every Version has and nothing else does —
  or it is refused with a sentence naming the problem, before any Deployment starts.
  "Remembered across runs" was already true: the Local Repo path rides in the persisted
  `InstallConfiguration` (ticket 03), now round-tripped in tests together with the new field.
- **`BuildConfiguration` joined `InstallConfiguration`** and is persisted
  (`build-configuration = release|debug`; anything else, including old settings files, reads
  as Release). The Core refuses Debug with any non-Local-Repo source in `plan` — the shell
  only *draws* the choice in Dev mode; the Core is what rules on it (rule 3). The
  `dev-mode()` shell rule is "a Local Repo has been named, in the field or remembered".
- **The Debug checkbox**, drawn under 43 Civs only in Dev mode; snapshot baselines
  re-rendered and reviewed (rule 15).

### Measured, against the real toolchain

```text
Debug build:      27 s full compile, 21,856,768-byte DLL (unoptimised — 2.1x the Release
                  size), own object directory beside the Release ones (real_build.rs)
Lua hot-reload:   edit a .lua in the checkout → redeploy → changed file in MODS in 114 ms,
                  DLL build skipped (real_install.rs — the ticket's end-to-end demo)
```

This also closes ticket 07's Debug/Release qualifier: the seam test
(`crates/core/tests/fingerprint.rs::the_debug_configuration_reaches_the_runner_and_has_its_own_fingerprint`)
now proves through the boundary that Debug is its own fingerprint (Release→Debug rebuilds,
unchanged Debug skips), and the real Debug compile above proves the flag set itself.

### Verified

Fast suite: checkout validation and the dirty-tree identity (`sources/tests/local_repo.rs` —
identity stable across reads, changed by an edit, the tree untouched); Debug rejection
outside Dev mode with nothing fetched, built, or written; Debug reaching the boundary;
Debug-only-in-Dev-mode in the shell's accessibility tree (`installer/tests/shell.rs`).
The `#[ignore]`d real tests as measured above.

### Post-review correction

`/code-review` flagged the shell's silent Debug→Release substitution in `configuration()` as
a rule-3 breach — deciding which Build Configuration is legal is the Core's ruling, not the
shell's. The shell now always sends the stored choice; the Core's `plan` refusal is the one
place the rule lives. (Every state reachable through the UI still behaves identically — the
checkbox only exists in Dev mode — so this was about where the decision lives, not what
users see.)

### Notes

- **No running-game guard** is a fact to preserve, not code to write: no code path asks
  whether Civilization V is running, and the spec wants it that way (deliberate, for the
  hot-reload loop). Recorded here so nobody "helpfully" adds one.
- The shell still has no folder *picker dialog* — the field is typed/pasted, as it has been
  since ticket 01. A native file dialog is cosmetic and can ride along with ticket 10 if
  wanted; validation, remembering, and everything behind the field is done.
- Sync replaces Claimed Folders wholesale on every Deployment, so the 114 ms redeploy copies
  the whole mod content again — fine at these sizes; per-file delta sync would be ticket-10+
  optimisation territory if ever needed.
