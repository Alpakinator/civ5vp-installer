# 06 — Real DLL build

**What to build:** The installer compiles the Built DLL itself from the selected Version's sources — the build orchestration ported from the proven docker-branch process into the installer (no Python): source list read from the `.civ5proj` at the selected Version, the exact clang-cl/lld settings that produce a DLL the game accepts, parallel compilation, incremental recompilation of only changed sources within a build, the 43-Civs define, and Release/Debug configurations.

**Blocked by:** 04 — Upstream Cache, 05 — Toolchain Bootstrap.

**Status:** ready-for-agent

- [ ] Source list parsed from the project file at the selected Version (not hardcoded); an added source file at a newer Version is picked up automatically
- [ ] Compiler and linker flags match the docker-branch build exactly for Release, Debug, and 43-Civs variants
- [ ] Full build of a real Version succeeds on Linux through the toolchain-runner boundary; resulting DLL is functionally equivalent to the docker-built reference (size/exports/imports comparison at minimum)
- [ ] Incremental rebuild after touching one source recompiles only affected objects and relinks
- [ ] Compile/link failures surface as plain-language errors with the full log saved
- [ ] Build feeds the Deployment so a real Version installs end-to-end with a genuinely built DLL
