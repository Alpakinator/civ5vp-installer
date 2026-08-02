# 01 — Walking skeleton: Core seam + minimal egui shell

**What to build:** A user can launch a plain window, click Install, and see `(1) Community Patch` (with a marker DLL) appear in a chosen MODS folder — end to end through the headless Core, fed by a fake source provider and a fake toolchain runner. This is the tracer bullet: the Rust workspace, the Core's plan→execute→progress API (the single primary seam from the spec), the two injected boundaries, and a thin unstyled egui shell over it.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Rust workspace builds one binary; the egui shell contains no logic beyond calling the Core
- [ ] Core accepts an Install Configuration + resolved folder paths and executes a plan against injected source-provider and toolchain-runner boundaries
- [ ] A Core-seam test deploys a CP-only configuration from a fixture repository into temp MODS/DLC/Text dirs and asserts the resulting file tree (marker DLL from the fake toolchain)
- [ ] Progress/results flow from Core to the shell (visible during the demo install)
- [ ] House test style established: external behavior only, fixtures + temp dirs, no reaching into Core internals
