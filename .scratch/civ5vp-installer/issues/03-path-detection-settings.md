# 03 — Path detection + settings persistence

**What to build:** On launch the installer finds the MODS, DLC, and Text Folders by itself — on Linux via the Steam library and Proton prefix, on Windows via known folders and the Steam registry — or falls back to a validated manual picker. A native Aspyr Linux port is detected only to be refused with a plain explanation that VP requires the Windows version under Proton. Detected/picked paths and the last Install Configuration are remembered in the App Data Store across runs.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

**Validation markers.** A folder counts as the real thing only if these are present:

- **Game Installation** — `CivilizationV.exe` and `CivilizationV_DX11.exe` at the root, plus `Assets/DLC/`. The DLC Folder *is* `<game>/Assets/DLC`.
- **Brave New World** — `Assets/DLC/Expansion2/` must exist. Vox Populi requires BNW; a game without it is a refusal case, not a warning.
- **Documents side** — `MODS/`, `Text/`, `ModUserData/`, and `UserSettings.ini`. The Text Folder is `<documents>/Text`.
- **Steam app id** is `8930`.

**Reference layout (a real Linux/Proton install, for fixtures):**

```
game:      <library>/steamapps/common/Sid Meier's Civilization V
documents: <library>/steamapps/compatdata/8930/pfx/drive_c/users/steamuser/
             Documents/My Games/Sid Meier's Civilization 5
```

Note the Documents folder says "Civilization 5" while the game folder says "Civilization V" — do not derive one from the other by string substitution.

- [x] Linux detection resolves game and Proton-prefix Documents paths from fixture Steam library layouts (multi-library `libraryfolders.vdf` included)
- [x] Native Linux port fixture is detected and refused with the explanation; no Deployment possible against it
- [x] Windows detection lives behind a thin platform adapter; its logic is exercised via the Core seam with fixture inputs (real-Windows verification deferred per the spec's platform constraint)
- [x] Manual picker validates a chosen folder against the markers below and rejects a wrong folder before anything is written, naming which marker was missing
- [x] Paths and last configuration persist in the App Data Store and pre-fill the next launch

## Comments

**Implemented.** `crates/core/src/detect/` is the whole of detection and validation;
`crates/core/src/settings.rs` is the App Data Store, the settings file and `start_up`, which
reconciles what was remembered with what can be found. The shell's four text fields became
three — the Community-Patch-DLL checkout, the **Civilization V game folder** and the
**Civilization 5 Documents folder** — with the MODS, DLC and Text Folders shown read-only
because the Core derives them; the player never picks the same thing twice. 45 tests pass (was
21): 14 detection, 10 settings, 8 shell, 8 deployment, 5 CLI. Clippy is clean with `-D warnings`.

What the platform adapter (`crates/core/src/detect/platform.rs`, the only `#[cfg]` in the Core)
actually does, and what it does not:

* **Linux** — the four usual Steam roots (`$XDG_DATA_HOME/Steam`, `~/.local/share/Steam`,
  `~/.steam/{steam,root}`, the Flatpak one), then `libraryfolders.vdf` for the rest. Verified
  against a real multi-library Proton install by `detection_finds_the_game_on_this_machine`,
  which is `#[ignore]`d because it needs the game installed (rule 14).
* **Windows** — environment variables only: `%ProgramFiles(x86)%\Steam` and `%ProgramFiles%\Steam`
  for the libraries, `%USERPROFILE%\Documents` and `%USERPROFILE%\OneDrive\Documents` for the
  Documents side. **The Steam registry key and `SHGetKnownFolderPath` are deferred**: both need
  `unsafe` or a `windows` crate dependency in a crate that has none, and neither could be
  verified without the Windows runner the spec says does not exist yet. A user who moved Steam
  or redirected Documents picks the folders by hand until then. The *judgement* about what the
  adapter finds is platform-agnostic and is exercised on Linux against a Windows-shaped fixture.

Deliberately left for later tickets:

* There is no file-picker dialog — the folders are typed or pasted. `rfd` is a dependency and a
  desktop-portal runtime dependency, and the ticket's requirement (validate a chosen folder,
  name the missing marker) is met without it. Worth revisiting with ticket 09's styling.
* The Install Configuration is remembered whole (source, Version, Flavor, EUI, 43 Civs) and
  round-tripped in `tests/settings.rs`, but the shell can still only *choose* the source folder
  — ticket 02 gives the Flavor and the toggles controls of their own.
* Nothing yet clears the App Data Store or reports its size (user story 25, ticket 10).
