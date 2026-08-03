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

- [ ] Linux detection resolves game and Proton-prefix Documents paths from fixture Steam library layouts (multi-library `libraryfolders.vdf` included)
- [ ] Native Linux port fixture is detected and refused with the explanation; no Deployment possible against it
- [ ] Windows detection lives behind a thin platform adapter; its logic is exercised via the Core seam with fixture inputs (real-Windows verification deferred per the spec's platform constraint)
- [ ] Manual picker validates a chosen folder against the markers below and rejects a wrong folder before anything is written, naming which marker was missing
- [ ] Paths and last configuration persist in the App Data Store and pre-fill the next launch
