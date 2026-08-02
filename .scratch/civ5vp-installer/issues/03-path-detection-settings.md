# 03 — Path detection + settings persistence

**What to build:** On launch the installer finds the MODS, DLC, and Text Folders by itself — on Linux via the Steam library and Proton prefix, on Windows via known folders and the Steam registry — or falls back to a validated manual picker. A native Aspyr Linux port is detected only to be refused with a plain explanation that VP requires the Windows version under Proton. Detected/picked paths and the last Install Configuration are remembered in the App Data Store across runs.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

- [ ] Linux detection resolves game and Proton-prefix Documents paths from fixture Steam library layouts (multi-library `libraryfolders.vdf` included)
- [ ] Native Linux port fixture is detected and refused with the explanation; no Deployment possible against it
- [ ] Windows detection lives behind a thin platform adapter; its logic is exercised via the Core seam with fixture inputs (real-Windows verification deferred per the spec's platform constraint)
- [ ] Manual picker validates that chosen folders plausibly belong to a Civ5 install and rejects wrong folders before anything is written
- [ ] Paths and last configuration persist in the App Data Store and pre-fill the next launch
