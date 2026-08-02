# 08 — Dev mode: Local Repo

**What to build:** A mod developer points the installer at their own Community-Patch-DLL checkout (folder picker, remembered, validated) and it builds and deploys the working tree exactly as-is — uncommitted changes included, zero git operations performed on it. Same Flavor/EUI/43-Civs options as the GitHub path, plus the Debug/Release choice exposed only here. Deploying while the game is running is permitted (the Lua/SQL hot-reload loop).

**Blocked by:** 02 — Deployment matrix, 06 — Real DLL build.

**Status:** ready-for-agent

- [ ] Local Repo selection validates the folder is a Community-Patch-DLL checkout and is remembered across runs
- [ ] Deployment content comes byte-for-byte from the working tree, uncommitted changes included; no git command ever runs against the Local Repo
- [ ] All Install Configurations work from a Local Repo; Debug/Release visible only in Dev mode
- [ ] Fingerprint-based skip works for the dirty working tree (via ticket 07 semantics)
- [ ] No running-game guard: deployment proceeds with the game open
- [ ] End-to-end demo: edit a Lua file in the checkout, redeploy, changed file is in MODS
