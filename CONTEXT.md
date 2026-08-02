# CONTEXT — VP Installer Ubiquitous Language

Glossary for the Vox Populi installer for Civilization V. Terms only — no implementation details.

## Terms

**Installation Source** — where the mod files and DLL sources come from. Exactly two kinds: the **Upstream Cache** (a local cached clone of `LoneGazebo/Community-Patch-DLL`) or a **Local Repo** (a developer's own checkout, used as-is, including uncommitted changes).

**Version** — the ref of the Community-Patch-DLL repository being installed. Three selectable tiers: a **Release** (a `Release-*` tag), the **Latest Development Version** (upstream `master` HEAD), or an **Arbitrary Ref** (any branch/tag/commit, advanced users only). A Local Repo's version is whatever its working tree currently contains.

**Flavor** — the base choice of what to install: **Community Patch only** (`(1)`) or **Vox Populi** (`(1)` + `(2)` + Squads + VPUI). Vox Populi implies Community Patch.

**EUI** — the Enhanced User Interface (`UI_bc1`, installed as DLC). A toggle that is only legal with the Vox Populi flavor. Selecting it also swaps in the EUI Compatibility Files (`(3a)`) and strips `LUA/` from `(1)` and `(2)`.

**43 Civs** — a toggle, legal with both Flavors and with or without EUI. Means: the Built DLL is compiled with the 43-civ setting and placed in `(1)` (the only DLL deployed), and the `(3b)` mod folder is deployed containing only its `.modinfo` and `AdvancedSetup.lua`.

**Squads** — the `(4a) Squads for VP` mod. Auto-included with the Vox Populi flavor; never a user-facing choice.

**Install Configuration** — the complete user selection: Installation Source + Version + Flavor + toggles (EUI, 43 Civs).

**Built DLL** — `CvGameCore_Expansion2.dll` compiled locally by the installer with clang. The installer always builds it; DLLs checked into the repository are never deployed (they are stale outside release commits).

**Toolchain** — everything needed to produce the Built DLL: portable clang/lld and the extracted Windows SDK 7.0 + VC9 CRT. Acquired by **Toolchain Bootstrap**: downloaded and unpacked on first build, then kept in the **Toolchain Cache**. Never bundled inside the installer executable.

**App Data Store** — the single installer-owned directory in the platform's app-data location (`%LOCALAPPDATA%` on Windows, XDG data dir on Linux) holding the Upstream Cache, Toolchain Cache, settings, and logs. The executable itself is a lone file that stores nothing beside itself. The UI exposes the store's location and size and a button that clears it (never touching the game).

**Build Fingerprint** — a hash of everything that determines the Built DLL: all source inputs at the selected Version (or the Local Repo's working files), compiler flags, the 43-Civs setting, and the Toolchain version. Recorded next to the deployed DLL together with the DLL's own hash; when both still match, the build is skipped.

**Upstream Cache** — the installer-managed clone of the upstream repository, fetched incrementally so no file content is ever downloaded twice. Checking out a Version happens here.

**Text Folder** — `…/Documents/My Games/Sid Meier's Civilization 5/Text/`. Receives `VPUI_tips_en_us.xml` (loading-screen tips) for every Vox Populi configuration. The third deployment target alongside the MODS and DLC Folders.

**MODS Folder** — the game's mod directory (`…/Documents/My Games/Sid Meier's Civilization 5/MODS`). Deployment target for mod folders `(1)`, `(2)`, `(3a)`, `(3b)`, `(4a)`.

**DLC Folder** — the game's DLC directory (`…/Sid Meier's Civilization V/Assets/DLC`). Deployment target for `VPUI` and `UI_bc1`.

**Claimed Folders** — the exact set of folders the installer owns and may create, sync, or delete: `(1) Community Patch`, `(2) Vox Populi`, `(3a) VP - EUI Compatibility Files`, `(3b) 43 Civs Community Patch`, `(4a) Squads for VP` in the MODS Folder; `VPUI` and `UI_bc1` in the DLC Folder. Nothing outside this set is ever touched.

**Sync** — how Deployment treats Claimed Folders: their contents are made to match the Install Configuration exactly (stale files deleted), and Claimed Folders not in the configuration are removed. After every Deployment the game's `cache` folder is cleared; `ModUserData` is preserved.

**Uninstall** — removing all Claimed Folders (and clearing `cache`), restoring an unmodded game.

**Game Installation** — the Steam install of the *Windows* version of Civilization V. On Linux this means the game running under Proton (MODS Folder lives inside the Proton prefix). The native Aspyr Linux port cannot load the Built DLL and is detected only to be refused with an explanation.

**Deployment** — copying the selected files from the Installation Source into the MODS and DLC Folders according to the Install Configuration, with the standard exclusions (project files, source art, docs, checked-in DLLs).
