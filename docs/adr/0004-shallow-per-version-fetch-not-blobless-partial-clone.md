# Upstream Cache fetches one Version shallowly, not a blobless partial clone

The spec's preferred strategy for the Upstream Cache was a **blobless partial clone**
(`--filter=blob:none`) through an embedded git library, with "full clone" and "ship a static
git" named as the fallbacks. `gix` (gitoxide) cannot do a blobless clone, so the Upstream
Cache instead does a **depth-1 shallow fetch per Version** into one accumulating repository.
Measured against the real upstream, that transfers *less* than the preferred strategy would
have, and it needs neither fallback.

## Why not blobless, concretely

Two independent blockers in `gix 0.74`:

1. **The `filter` line is never sent.** `gix-protocol` has `fetch::Arguments::filter()`, but
   nothing in `gix`'s fetch path calls it, and `gix::remote::ref_map::Options` has no way to
   ask for one. Requesting a partial clone means driving `gix-protocol`'s handshake,
   negotiation and pack receipt by hand.
2. **There is no promisor support.** The string `promisor` does not appear anywhere in `gix`
   or `gix-protocol`. A blobless clone is only useful because checkout can fetch the blobs it
   is missing on demand; without that, the clone would produce a repository whose files can
   never be written out.

`git2`/libgit2 was not tried: it pulls a C toolchain (rule 17) and its partial-clone support
is weaker still.

## What each strategy actually costs

Measured against `LoneGazebo/Community-Patch-DLL` (~4.5 GB of history) with the `git` CLI,
which *can* do all of these, so the strategies could be compared before one was implemented.
"Wire" is git's own `Receiving objects: … done.` figure.

| | wire | note |
| --- | --- | --- |
| Full clone | ~4.5 GB | the documented first fallback |
| Blobless clone, no checkout | 10.2 MiB | commits + trees only - the history is almost entirely blobs |
| …then checkout `master` | +~149 MiB | the blobs of one snapshot, fetched lazily |
| **Shallow `--depth 1` clone of `master`** | **147.7 MiB** | one snapshot, commits and trees included |
| Shallow switch to `Release-5.2` | 14.5 MiB | |
| Shallow switch to `Release-4.15` | 25.9 MiB | |
| Shallow switch to `Release-3.0` | 9.0 MiB | |

Blobless and shallow land within a few percent of each other for a first materialization, and
both are an order of magnitude below the provisional 1.5 GB ceiling. Switching Version costs
tens of megabytes either way. Shallow wins on the only axis that separates them: `gix` can do
it today.

## Decision

The Upstream Cache is **one repository that accumulates a depth-1 snapshot per Version**:

- every materialized Version keeps its own local ref under `refs/civ5vp/`, and nothing is ever
  pruned. Those refs are what the next fetch offers the server as "already have", which is why
  a switch transfers a fraction of a first fetch rather than a second whole snapshot -
  `CONTEXT.md`'s "no file content is ever downloaded twice";
- the working tree is emptied and rewritten from the selected commit rather than updated in
  place, because `gix` has no equivalent of `git checkout`'s stale-file removal, and because
  a directory that is refilled from scratch cannot keep a file from the Version before it;
- a commit id is recorded inside `.git` only once every file of that Version is written, so an
  interrupted checkout is redone rather than trusted.

## Consequences

- **The cache is not a browsable git repository.** Its history is one commit deep per Version,
  so `git log` in it shows nothing useful. It is a cache, and the UI presents it as one.
- **Every Version switch is a fetch.** There is no local checkout of a Version that was never
  fetched, because its blobs were never downloaded. Repeat installs of the *same* Version skip
  the checkout but still ask the remote whether anything changed.
- **Arbitrary Refs by commit id depend on the server allowing want-by-oid.** GitHub does;
  a self-hosted mirror might not.
- **Blobless remains the better long-term shape** if `gix` grows filter and promisor support:
  it would keep full history metadata, so switching Version would not need a network round trip
  to resolve a ref. Revisiting is a fetch-strategy change behind `UpstreamCache`, not an API
  change.
- Rule 5 holds either way: no fallback to a shipped or bootstrapped `git` binary was needed,
  and nothing in the installer runs an external process.
