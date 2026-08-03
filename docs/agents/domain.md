# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

This repo is **single-context**: one `CONTEXT.md` + `docs/adr/` at the root.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — the ubiquitous language for the installer. Terms only, no implementation.
- **`docs/adr/`** — read ADRs that touch the area you're about to work in. Currently:
  - `0001` — always build the DLL with a bootstrapped toolchain
  - `0002` — Rust + egui over a webview
  - `0003` — embed Tw Cen MT
- **`docs/spec.md`** — the full spec (problem, user stories, implementation decisions, testing decisions, out of scope). Read it before any implementation work; the tickets under `.scratch/` are slices of it.

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) creates them lazily when terms or decisions actually get resolved.

## File structure

```
/
├── AGENTS.md                          ← agent entry point
├── CODING_STANDARDS.md                ← the non-negotiable invariants
├── CONTEXT.md                         ← glossary
├── docs/
│   ├── spec.md
│   ├── adr/
│   └── agents/                        ← this directory
└── .scratch/civ5vp-installer/issues/  ← tickets
```

## Use the glossary's vocabulary

When your output names a domain concept (in a ticket title, a type name, a test name, a refactor proposal), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary avoids — write `Claimed Folders`, not "our folders"; `Sync`, not "copy step"; `Built DLL`, not "the compiled dll"; `Install Configuration`, not "user settings".

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0001 (always build the DLL) — but worth reopening because…_
