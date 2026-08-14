# Ticket 13: Unofficial versions in the picker

Status: done

## What the user asked for

> 'latest development version' is not available by default, only official releases. add a
> button for 'unofficial versions' toggled off by default. when toggled on, the dropdown
> will have extra versions corresponding to commits, named e.g. 5.4.2.01 - xxxxxxx for
> each commit after the release, where 01 is the order and xxxxxxx the start of the commit
> message. these messages are always longer than the dropdown — how do we solve this?

Scope settled with the user: commits **since the newest release only** (the recommended
option) — historic between-release segments would cost dozens of API calls and thousands
of dropdown rows for archaeology nobody does. Width: rows truncate the summary at 44
chars with an ellipsis; the full message (plus a short hash) is the hover text; the
closed combo shows just the number.

## How it works

- `Version::UnofficialBuild { label, commit }` — a fourth Version tier (CONTEXT.md
  updated). The label (`5.4.3.07`) is what the settings remember, what the fingerprint
  records, and what the DLL carries as its version string; the commit hash is what
  installs, so the pick stays pinned however far upstream moves after listing.
- Listing: `SourceProvider::unofficial_versions(newest_release)` — the Upstream Cache is
  a *shallow* clone with no history to walk, so the list comes from one GitHub compare
  call (`/compare/Release-X...master`, ureq + serde_json; endpoint derived from the git
  URL, overridable for mirrors). Commits are numbered oldest-first `NN = 01…`; at most
  250 per page, with a progress line when upstream is further ahead.
- Install: the recorded hash rides the existing fetch-by-commit-id path
  (`refs/civ5vp/unofficial/<sha>`), proven live by `real_upstream.rs`.
- UI: "Latest development version" removed from the offer (the newest unofficial build IS
  master, with an honest name; remembered old configurations still restore). The toggle
  is off by default, not persisted — but a remembered unofficial pick turns it back on.
  The lookup runs on a thread once the toggle is on and the catalog has named the newest
  Release, with the usual Fetching/Failed/retry states.

## Coverage

Unit: compare-JSON parsing (numbering, first-line summaries, escapes, garbage). Settings:
UnofficialBuild round-trip. Shell: default offer has neither development nor unofficial
entries; toggling lists them newest-first with the long summary truncated (ellipsis
asserted); a pick shows as `5.2.01`, survives a relaunch, and re-enables the toggle.
Live (`--ignored`): real compare call + real materialize by hash. At the time of landing,
master sat exactly at Release-5.4.3 — the empty list was verified correct against the API
and confirmed by the user.
