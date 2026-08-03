# 01 — Walking skeleton: Core seam + minimal egui shell

**What to build:** A user can launch a plain window, click Install, and see `(1) Community Patch` (with a marker DLL) appear in a chosen MODS folder — end to end through the headless Core, fed by a fake source provider and a fake toolchain runner. This is the tracer bullet: the Rust workspace, the Core's plan→execute→progress API (the single primary seam from the spec), the two injected boundaries, and a thin unstyled egui shell over it.

The tracer bullet also establishes how the UI is **seen**, not just how it is tested for behavior: an `egui_kittest` harness that renders screens to PNG snapshots, and a headless screenshot mode on the real binary. Ticket 09 restyles this shell into the Civ5 skin and its last acceptance criterion is a visual comparison — that comparison needs a mechanism, and the cheapest moment to build it is now, while there is one plain screen to render rather than eight styled ones.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [x] Rust workspace of (at least) two crates — a Core **library** crate and a binary crate holding the egui shell — building one binary. The Core crate's `Cargo.toml` must not list `egui`/`eframe`, so the seam is a compile error to cross, not a convention
- [x] The egui shell contains no logic beyond calling the Core and rendering what it returns
- [x] Core accepts an Install Configuration + resolved folder paths and executes a plan against injected source-provider and toolchain-runner boundaries
- [x] A Core-seam test deploys a CP-only configuration from a fixture repository into temp MODS/DLC/Text dirs and asserts the resulting file tree (marker DLL from the fake toolchain)
- [x] Progress/results flow from Core to the shell (visible during the demo install)
- [x] House test style established: external behavior only, fixtures + temp dirs, no reaching into Core internals
- [x] `egui_kittest` wired up with its `wgpu` + `snapshot` features and a `kittest.toml`; at least one test drives the shell through its AccessKit tree (find the Install button by label, click it, assert the resulting state) and one renders the screen to a committed baseline under `tests/snapshots/`, failing on visual drift
- [x] The binary supports a headless render mode — `cargo run -- --screenshot <dir>` with size and DPI-scale options — writing one PNG per screen via `egui::ViewportCommand::Screenshot`, with no user present
- [x] `docs/ui-reference/` created with a README stating what belongs there: Civ5 captures used as visual reference only, never shipped (ADR-0003)
- [x] `AGENTS.md`'s Commands section is true — every command listed there runs

## Comments

**Implemented.** The workspace is `crates/core` (headless Core, zero dependencies, no egui in its
`Cargo.toml`) and `crates/installer` (egui shell + binary + the render harness). 15 tests pass:
7 Core-seam tests over a committed miniature repository fixture, 5 CLI tests, and 3 shell tests
(two driving the AccessKit tree, one comparing four committed PNG baselines). Clippy is clean
with `-D warnings`.

Deliberately left for later tickets, and visible rather than hidden:

* `Flavor::VoxPopuli` is rejected at plan time with a plain-language message — ticket 02 replaces
  that with the real deployment matrix. Everything the walking skeleton *does* deploy is asserted.
* The binary ships two stand-ins (`crates/installer/src/placeholder.rs`): a source provider that
  serves a Local Repo folder as-is and refuses the Upstream Cache (ticket 04), and a toolchain
  runner that writes a marker file instead of compiling (tickets 05/06). The UI says so on screen.
* Exclusions during Deployment cover checked-in DLLs (ADR-0001) and ModBuddy project files only;
  the rest come from the InnoSetup script in ticket 02.
* The Core's work directory is a temp directory; the App Data Store arrives with ticket 03.
* The game `cache` clear and the Text Folder deployment belong to ticket 02 and are not faked here.
