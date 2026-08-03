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

The workspace is `crates/core` (the headless Core, a library), `crates/installer` (the egui shell and the binary) and `crates/toolchain` (Toolchain Bootstrap: pinned downloads and in-process ISO/MSI/CAB extraction). All are default workspace members, so a bare `cargo test` is the whole suite and `cargo run` means the installer.

Standard `cargo` commands apply. Only the non-obvious ones are listed:

```bash
cargo test -- --ignored           # real-toolchain / real-clone integration tests (slow, on demand)
cargo run -- --screenshot <dir>   # render screens headlessly to PNG (see "UI work")
UPDATE_SNAPSHOTS=true cargo test  # accept changed egui_kittest snapshot baselines — review the diffs first
```

`--screenshot` takes `--size <WxH>` and `--scale <factor>`, each repeatable; every screen is rendered at every combination. `cargo run -- --help` lists them.

### The real Toolchain Bootstrap (ticket 05)

`crates/toolchain/tests/real_bootstrap.rs` is the one `#[ignore]`d test that exists so far. It
downloads the real pinned artifacts (~1.6 GB: the Windows SDK 7.0 ISO from archive.org and the
portable LLVM 18.1.8 from GitHub), extracts the four ISO members in process, applies the six
Linux fix-ups, and checks that every name in `docs/pinned-artifacts.md` §4 resolves.

```bash
# Keep the 1.6 GB somewhere that survives `cargo clean`, or it downloads again next time.
CIV5VP_TOOLCHAIN_CACHE=~/.cache/civ5vp-toolchain \
  cargo test -p civ5vp-toolchain -- --ignored --nocapture
```

Without `CIV5VP_TOOLCHAIN_CACHE` it uses `target/tmp/toolchain-bootstrap`. The archive.org
download runs at a few MB/s and the image is 1.45 GiB, so budget well over an hour; it
resumes, and verified downloads are reused, so an interrupted run is cheap to repeat.
`--nocapture` is what makes the progress visible. Use `--release` — the LZX decompression is
many times slower in a debug build.

To look at an image you already have without extracting it — which members are present, what
each MSI's layout says, how the cabinets are structured:

```bash
CIV5VP_SDK_ISO=/path/to/GRMSDK_EN_DVD.iso \
  cargo test -p civ5vp-toolchain --lib -- --ignored --nocapture inspect_a_real_disc_image
```

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
