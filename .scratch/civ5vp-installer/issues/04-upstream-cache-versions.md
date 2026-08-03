# 04 — Upstream Cache: real git fetch + version tiers

**What to build:** The user picks a Version from three tiers — Releases (`Release-*` tags), latest development (`master`), or an arbitrary ref — and the installer materializes it in the Upstream Cache inside the App Data Store, downloading incrementally so no file content is ever fetched twice. This ticket resolves the spec's riskiest bet: blobless partial clone via an embedded git library, with the documented fallbacks (full clone, or shipping a static git) if it proves unreliable — record the outcome as an ADR if the fallback is taken.

**Blocked by:** 01 — Walking skeleton.

**Status:** ready-for-agent

- [x] Version picker lists real Releases and latest master from the upstream repository; arbitrary ref accepted via advanced input
- [x] First materialization transfers under 250 MB on the wire, against ~4.5 GB of full history; a subsequent Version switch transfers under 75 MB. Both are measured, not estimated. Measured 2026-08-03: **147.7 MiB** first materialization, **32.7 MiB** for `master` → `Release-4.15`
- [x] Checkout of the selected Version feeds the Core's source-provider boundary; a real Release installs end-to-end (fake toolchain still supplying the DLL)
- [x] Network failure mid-fetch leaves the cache consistent and the game untouched; retry succeeds
- [x] Works without git installed on the machine
- [x] Clone-strategy decision (blobless / fallback) verified against the real upstream and recorded

## Comments

**What was built.** A new library crate, `crates/sources`, holding both Installation Sources
behind the Core's `SourceProvider` boundary: `UpstreamCache` (the managed clone) and the Local
Repo path, which is handed back byte-for-byte with no git operation run against it. Git work is
done in-process by `gix`; nothing shells out, so the installer works on a machine that has
never had git installed.

**Clone strategy — ADR-0004.** Blobless partial clone is not possible with `gix`: it never
sends a `filter` line, and it has no promisor support, so the blobs a checkout needs could
never be filled in. Neither documented fallback was taken. Instead the cache does a **depth-1
shallow fetch per Version** into one accumulating repository, keeping a local ref per Version
so the next fetch can offer them as "already have". Measured with the `git` CLI before
implementing, a blobless clone plus a checkout of `master` costs ~159 MiB and a shallow clone
of `master` costs 147.7 MiB — so the strategy `gix` can actually do is also the cheaper one.
Full clone, the first documented fallback, is ~4.5 GB.

**How the transfer figures were measured.** By the growth of the cache's object store across a
materialization, in the `#[ignore]`d test
`a_first_materialization_and_a_version_switch_stay_within_budget`. That is an upper bound on
wire bytes — the pack is written as it arrives and thin-pack completion can only add to it —
so a number below the ceiling proves the wire traffic was below it too. The independent `git`
CLI figure for the same shallow clone (147.66 MiB "Receiving objects") matches to within a
tenth of a MiB, which is the cross-check that the bound is tight.

**Ceilings.** Tightened from 1.5 GB / 250 MB to 250 MB / 75 MB, per this ticket's instruction.
The margin is deliberate rather than a round number: the first-fetch figure is a *snapshot* of
the repository, so it grows as the mod gains files (not as history gets longer), and a switch
between two Versions further apart than the year between `master` and `Release-4.15` costs
more than 32.7 MiB. Re-running the ignored tests with `--nocapture` reprints both numbers.

**Deferred.**

- *No offline fast path.* Re-installing a Version that is already unpacked skips the checkout
  but still asks the remote whether the ref moved. Making an immutable Release skip the network
  entirely belongs with the settings/App Data Store work, not here.
- *No progress bytes.* Fetch and checkout report stage messages through `ProgressReporter`, but
  not a percentage — `gix` wants a `prodash` progress tree for that, and the UI has nowhere to
  show it until ticket 09.
- *No cache size / clear-data surface.* `CONTEXT.md` puts that on the App Data Store (ticket
  03); `UpstreamCache::root()` is what it will need.
- *`AGENTS.md` line now stale.* "There are no `#[ignore]`d tests yet — tickets 04, 05 and 06
  add the first" is no longer true; ticket 04's are in place and documented in a new section at
  the end of the file. The stale sentence was left alone because other agents are editing that
  file this round.

**Fast suite needs `git` on the test machine, the installer does not.** The fixture
repositories are built with `gix`, but `gix`'s `file://` transport spawns `git-upload-pack` to
serve them. The installer only ever speaks `https` to GitHub, which `gix` does in-process, so
rule 5 holds where it matters — this is a property of the fixtures.
