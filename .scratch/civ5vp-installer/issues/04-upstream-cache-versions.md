# 04 — Upstream Cache: real git fetch + version tiers

**What to build:** The user picks a Version from three tiers — Releases (`Release-*` tags), latest development (`master`), or an arbitrary ref — and the installer materializes it in the Upstream Cache inside the App Data Store, downloading incrementally so no file content is ever fetched twice. This ticket resolves the spec's riskiest bet: blobless partial clone via an embedded git library, with the documented fallbacks (full clone, or shipping a static git) if it proves unreliable — record the outcome as an ADR if the fallback is taken.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

- [ ] Version picker lists real Releases and latest master from the upstream repository; arbitrary ref accepted via advanced input
- [ ] First materialization downloads roughly one working tree's worth, not full history; switching Versions fetches only changed content
- [ ] Checkout of the selected Version feeds the Core's source-provider boundary; a real Release installs end-to-end (fake toolchain still supplying the DLL)
- [ ] Network failure mid-fetch leaves the cache consistent and the game untouched; retry succeeds
- [ ] Works without git installed on the machine
- [ ] Clone-strategy decision (blobless / fallback) verified against the real upstream and recorded
