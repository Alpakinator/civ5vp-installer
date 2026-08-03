# AGENTS.md — Civ 5 VP Installer

A single-file desktop installer (Windows + Linux) for the Community Patch / Vox Populi mods for Civilization V. Rust + egui, one static binary, no runtime dependencies. It fetches sources incrementally, **always compiles the game DLL itself** with a bootstrapped clang toolchain, and deploys deterministically into the game's MODS, DLC, and Text Folders.

## Read before working

| File | What it is |
| --- | --- |
| `docs/spec.md` | The spec — problem, 34 user stories, implementation decisions, testing decisions, out of scope. **Read this first.** |
| `CONTEXT.md` | Ubiquitous language. Use these terms exactly; they are the type names. |
| `CODING_STANDARDS.md` | The non-negotiable invariants, each traced to the spec or an ADR. |
| `docs/adr/` | Decisions and their reasoning. ADR-0001 (always build the DLL), ADR-0002 (Rust+egui), ADR-0003 (embed Tw Cen MT). |
| `.scratch/civ5vp-installer/issues/` | The ten tickets, with blocking edges. Start at `01-walking-skeleton.md`. |

## Commands

The workspace is `crates/core` (the headless Core, a library) and `crates/installer` (the egui shell and the binary). Both are default workspace members, so a bare `cargo test` is the whole suite and `cargo run` means the installer.

Standard `cargo` commands apply. Only the non-obvious ones are listed:

```bash
cargo test -- --ignored           # real-toolchain / real-clone integration tests (slow, on demand)
cargo run -- --screenshot <dir>   # render screens headlessly to PNG (see "UI work")
UPDATE_SNAPSHOTS=true cargo test  # accept changed egui_kittest snapshot baselines — review the diffs first
```

`--screenshot` takes `--size <WxH>` and `--scale <factor>`, each repeatable; every screen is rendered at every combination. `cargo run -- --help` lists them.

There are no `#[ignore]`d tests yet — tickets 04, 05 and 06 add the first real-clone and real-toolchain ones. The command runs and reports nothing until then.

Run clippy and the fast suite regularly; the full suite once before handing work back.

## How this project is verified

Development and routine verification happen on **Arch Linux only**. There is no Windows machine and no CI yet. That constraint is why the Core seam exists and why platform adapters must stay thin — see `CODING_STANDARDS.md` rules 4, 12–14.

"Tests pass" means the fast suite with faked boundaries. It does **not** mean the real clone, the real SDK extraction, or a real DLL build works. Say which one you actually ran.

## UI work

The installer must look like part of Civilization V (ticket 09). Visual work is a render-and-look loop, not a guess:

1. `egui_kittest` drives the UI through its AccessKit tree and renders screens to PNG snapshots under `tests/snapshots/`.
2. `cargo run -- --screenshot <dir>` renders the real binary's screens at several sizes and DPI scales.
3. Open the PNGs, compare against the Civ5 reference images in `docs/ui-reference/`, change the theme, re-render.

Never report a visual change as done without looking at a rendered image of it.

## Agent skills

### Issue tracker

Local markdown — one file per ticket under `.scratch/<feature>/issues/`, no remote, no `gh`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical roles, unchanged, recorded as a `**Status:**` line in each ticket file. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Real-clone integration tests (ticket 04)

`crates/sources` holds the Installation Sources — the Upstream Cache and the Local Repo. Its
fast tests fetch from fixture repositories built in a temp directory, so they never touch the
network. The tests that talk to the real `LoneGazebo/Community-Patch-DLL` are `#[ignore]`d:

```bash
# ~80s once built; ~600 MB downloaded, ~1.8 GB left under target/tmp. Prints the transfer figures.
cargo test --release -p civ5vp-sources --test real_upstream -- --ignored --nocapture --test-threads 1
```

`--nocapture` is the point of running them: the printed byte counts are the evidence behind
the transfer budget in ticket 04, and re-running is how that budget is re-checked. They write
into `target/tmp/`, not `/tmp`, because a snapshot of the repository is larger than a RAM disk
should hold.

Two things about them worth knowing before changing either:

- The fixture repositories are built with `gix`, but `gix`'s `file://` transport spawns
  `git-upload-pack`, so **the fast suite needs `git` on the machine running it**. The installer
  itself never uses `file://` — it speaks `https` to GitHub in-process — so this is a property
  of the fixtures, not a runtime dependency (rule 5).
- The clone strategy and the numbers behind it are ADR-0004.
