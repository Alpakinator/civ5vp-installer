# 10 — Release readiness

**What to build:** Someone who has never seen this conversation downloads one file and successfully installs Vox Populi. Storage panel shows the App Data Store location and size with a working clear-data button (never touching the game); logs are saved with copy/open buttons wherever errors surface; launch pings for a newer installer release and shows a notification link (no auto-update); CI builds the two single-file binaries (Windows exe, Linux binary); README covers download, first run, and the ~5 GB footprint honestly.

**Blocked by:** 02, 03, 07, 08, 09.

**Status:** ready-for-agent

- [ ] Storage panel reports real location/size; clear-data empties the App Data Store and the next install re-bootstraps cleanly
- [ ] All failure surfaces route through the plain-language panel with log save/copy/open
- [ ] New-version notification appears when a newer release exists; absent otherwise; launch works offline
- [ ] CI produces both binaries from a clean checkout; Linux binary verified locally, Windows binary built by CI (verification deferred per the spec's platform constraint)
- [ ] README with install/usage instructions and footprint disclosure
- [ ] Fresh-machine walkthrough (clean App Data Store, empty MODS) succeeds start to finish
