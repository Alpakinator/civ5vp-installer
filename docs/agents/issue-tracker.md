# Issue tracker: Local Markdown

This repo has **no remote**. Issues and specs live as markdown files in this repo — there is no GitHub/GitLab tracker to call, and `gh issue` commands do not apply here.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`. The only feature so far is `.scratch/civ5vp-installer/`.
- Implementation issues are one file per ticket at `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01` — never a single combined tickets file.
- Triage state is recorded as a `**Status:**` line near the top of each issue file (see `triage-labels.md` for the role strings).
- Blocking edges are recorded as a `**Blocked by:**` line near the top, referencing other tickets by number and name.
- Acceptance criteria are a `- [ ]` checklist at the bottom of the file.
- Comments and conversation history append to the bottom of the file under a `## Comments` heading.

### Deviation from the default layout

The spec lives at **`docs/spec.md`**, not `.scratch/<feature-slug>/spec.md`. It is a first-class, committed project document alongside `CONTEXT.md` and `docs/adr/` — the tickets under `.scratch/` all derive from it. When a skill says "fetch the originating spec/PRD", read `docs/spec.md`.

## When a skill says "publish to the issue tracker"

Create a new file under `.scratch/<feature-slug>/issues/` (creating the directory if needed), following the conventions above. Commit it.

## When a skill says "fetch the relevant ticket"

Read the file at `.scratch/civ5vp-installer/issues/<NN>-*.md`. The user will normally pass the ticket number (e.g. "ticket 03") or the path directly.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a file with one **child** file per ticket.

- **Map**: `.scratch/<effort>/map.md` — the Notes / Decisions-so-far / Fog body.
- **Child ticket**: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`grilling`/`task`); a `Status:` line records `claimed`/`resolved`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `resolved`.
- **Frontier**: scan `.scratch/<effort>/issues/` for files that are open, unblocked, and unclaimed; first by number wins.
- **Claim**: set `Status: claimed` and save before any work.
- **Resolve**: append the answer under an `## Answer` heading, set `Status: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.
