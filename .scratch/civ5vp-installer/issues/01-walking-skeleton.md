# 01 — Walking skeleton: Core seam + minimal egui shell

**What to build:** A user can launch a plain window, click Install, and see `(1) Community Patch` (with a marker DLL) appear in a chosen MODS folder — end to end through the headless Core, fed by a fake source provider and a fake toolchain runner. This is the tracer bullet: the Rust workspace, the Core's plan→execute→progress API (the single primary seam from the spec), the two injected boundaries, and a thin unstyled egui shell over it.

The tracer bullet also establishes how the UI is **seen**, not just how it is tested for behavior: an `egui_kittest` harness that renders screens to PNG snapshots, and a headless screenshot mode on the real binary. Ticket 09 restyles this shell into the Civ5 skin and its last acceptance criterion is a visual comparison — that comparison needs a mechanism, and the cheapest moment to build it is now, while there is one plain screen to render rather than eight styled ones.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Rust workspace of (at least) two crates — a Core **library** crate and a binary crate holding the egui shell — building one binary. The Core crate's `Cargo.toml` must not list `egui`/`eframe`, so the seam is a compile error to cross, not a convention
- [ ] The egui shell contains no logic beyond calling the Core and rendering what it returns
- [ ] Core accepts an Install Configuration + resolved folder paths and executes a plan against injected source-provider and toolchain-runner boundaries
- [ ] A Core-seam test deploys a CP-only configuration from a fixture repository into temp MODS/DLC/Text dirs and asserts the resulting file tree (marker DLL from the fake toolchain)
- [ ] Progress/results flow from Core to the shell (visible during the demo install)
- [ ] House test style established: external behavior only, fixtures + temp dirs, no reaching into Core internals
- [ ] `egui_kittest` wired up with its `wgpu` + `snapshot` features and a `kittest.toml`; at least one test drives the shell through its AccessKit tree (find the Install button by label, click it, assert the resulting state) and one renders the screen to a committed baseline under `tests/snapshots/`, failing on visual drift
- [ ] The binary supports a headless render mode — `cargo run -- --screenshot <dir>` with size and DPI-scale options — writing one PNG per screen via `egui::ViewportCommand::Screenshot`, with no user present
- [ ] `docs/ui-reference/` created with a README stating what belongs there: Civ5 captures used as visual reference only, never shipped (ADR-0003)
- [ ] `AGENTS.md`'s Commands section is true — every command listed there runs
