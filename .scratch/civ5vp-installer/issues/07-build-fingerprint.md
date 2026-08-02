# 07 — Build Fingerprint + skip logic

**What to build:** Repeat installs are near-instant: the whole build is skipped when the Build Fingerprint (hash of all source inputs, compiler flags, 43-Civs setting, and Toolchain version) matches the sidecar recorded at last Deployment AND the deployed DLL's own hash still matches the recorded output. Any changed source, flag, or toolchain — or a manually swapped DLL — forces a rebuild. No false skips, no needless rebuilds.

**Blocked by:** 06 — Real DLL build.

**Status:** ready-for-agent

- [ ] Unchanged inputs + intact deployed DLL → build skipped (verified through the Core seam)
- [ ] Each of these alone forces a rebuild: edited source file, different configuration (Debug/Release, 43-Civs), different Toolchain version, tampered/replaced deployed DLL, missing sidecar
- [ ] Fingerprint for a checked-out Version derives from the git tree; for a dirty Local Repo it derives from working-file contents
- [ ] Skip decision is reported to the user ("DLL already up to date")
