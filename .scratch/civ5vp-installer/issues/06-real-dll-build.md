# 06 — Real DLL build

**What to build:** The installer compiles the Built DLL itself from the selected Version's sources — the build orchestration ported from the proven docker-branch process into the installer (no Python): source list read from the `.civ5proj` at the selected Version, the exact clang-cl/lld settings that produce a DLL the game accepts, parallel compilation, incremental recompilation of only changed sources within a build, the 43-Civs define, and Release/Debug configurations.

**Blocked by:** 04 — Upstream Cache, 05 — Toolchain Bootstrap.

**Status:** done

- [x] Source list parsed from the project file at the selected Version (not hardcoded); an added source file at a newer Version is picked up automatically
- [x] Compiler and linker flags match the docker-branch build exactly for Release, Debug, and 43-Civs variants — **with two Version-tracking additions the docker branch predates; see Comments**
- [x] Full build of a real Version succeeds on Linux through the toolchain-runner boundary; resulting DLL is functionally equivalent to the docker-built reference (size/exports/imports comparison at minimum) — **with a qualifier on what "the reference" is; see Comments**
- [x] Incremental rebuild after touching one source recompiles only affected objects and relinks
- [x] Compile/link failures surface as plain-language errors with the full log saved
- [x] Build feeds the Deployment so a real Version installs end-to-end with a genuinely built DLL

## Comments

Landed as a `build` module inside `crates/toolchain` (`build/{flags,project,invoke,state,mod}.rs`),
behind the existing `ToolchainRunner` boundary — `BootstrappedToolchain::build_dll` now
bootstraps (36 µs from a populated cache) and compiles. The Core gained two `BuildRequest`
fields: `build_configuration` (Release/Debug — the Core passes Release until ticket 08's Dev
mode) and `version_label` (feeds the generated `commit_id.inc`, below). The installer binary is
now wired to the real boundaries (`crates/installer/src/wiring.rs`); the shell tests keep the
placeholder fakes, which is what keeps the fast suite offline.

### Measured, on a real checkout (the ticket-04 Upstream Cache materialization at `target/tmp/real-budget`)

```text
clean full build:   63 s — shim, VC9 stubs, PCH, 172 sources in parallel, lld-link
built DLL:          10,279,424 bytes, PE32 i386 DLL
reference DLL:       9,655,808 bytes (checked into the same Version) — size ratio 1.06
imports:            identical DLL lists, byte for byte, including VERSION.dll and dbghelp.dll
exports:            exactly the .def contract (DllGetGameContext)
incremental:        touch one .cpp → exactly 1 of 172 objects recompiled + relink, 1.9 s
no-op rebuild:      ~10 ms, zero tool invocations (verified by the fake-invoker suite too)
43-Civs variant:    real build passes (84 s), own object directory, plain Release objects untouched
end-to-end:         real Deployment through wiring::core_at — fetch → build → Sync — deploys the
                    genuinely built DLL into (1) Community Patch in 69 s (real_install.rs)
```

`#[ignore]`d tests: `crates/toolchain/tests/real_build.rs` (full build + reference comparison,
43-Civs, incremental) and `crates/installer/tests/real_install.rs` (end-to-end Deployment).
All driven by `CIV5VP_DLL_SOURCE_ROOT` (a checkout) and `CIV5VP_TOOLCHAIN_CACHE`; commands in
each file's header. The fast suite drives the whole orchestration through a fake tool invoker
— staleness, parallelism, log shape, failure surfacing — and never starts a process.

### What "matches the docker branch exactly" turned out to mean

The docker branch froze both its source list **and** its flags, and upstream moved. Three
concrete instances, all found by pointing the build at a real newer Version:

1. **The frozen source list is stale** — `stackwalker/StackWalker.cpp` is in today's project
   file and not in the script. Parsing `CvGameCoreDLL_Expansion2/VoxPopuli.vcxproj` (present at
   every Release tag checked, back through Release-3.10.1) was the ticket's own premise;
   confirmed necessary, not merely nice.
2. **`STACKWALKER` must be defined at newer Versions and must not be at older ones.** The
   define guards a `#include` of a file old Versions do not have. It is read from the project
   file's `<PreprocessorDefinitions>` — the Version's own declaration — and lands where
   upstream's current clang scripts put it (after `EXTERNAL_PAUSING`). Every other predef is
   the docker transcription, including `VPRELEASE_ERRORMSG`, which upstream's scripts dropped.
3. **Newer Versions trimmed the VC9 CRT stubs out of `clang.cpp`** (their Windows clang builds
   link a modern CRT), so `__std_terminate` came up undefined at link. The installer now
   generates `vc9-crt-compat.cpp` — the stubs transcribed from the docker branch's own
   `clang.cpp` — into the object directory and links it always; at old Versions whose
   `clang.cpp` still carries them, the duplicates are exactly what the reference's
   `/FORCE:MULTIPLE` tolerates. Relatedly, StackWalker pulls `version.lib` via
   `#pragma comment(lib, …)`, the one channel `/NODEFAULTLIB:VERSION` blocks — so when the
   Version compiles StackWalker, `version.lib` is named explicitly in the response file
   (explicit naming is unaffected by `/NODEFAULTLIB`).

The flag criterion is ticked in that spirit: the docker configuration is transcribed verbatim
(`flags.rs` asserts the full Release vector literally), and the two deviations above exist
because the docker branch cannot build newer upstream at all without them.

### Qualifier on "the docker-built reference"

No docker image was run. The comparison baseline is the **maintainer-built DLL checked into
the same Version** (`(1) Community Patch/CvGameCore_Expansion2.dll`) — the binary players get
from the official installer, which is a stronger acceptance target than a docker rebuild, but
not literally what the criterion names. The reference exports one extra symbol beyond the
`.def` contract (`std::_Init_locks::operator=`, VC9 CRT lock plumbing its compiler chose to
re-export); the comparison tolerates exactly that and nothing else. Byte identity was never
expected — different compiler binary, different vintage.

### Design notes

- **Incremental = mtime + the PCH as dependency fence.** Any project header newer than the
  PCH rebuilds the PCH; a rebuilt PCH is newer than every object, so all objects follow. No
  per-file dependency tracking — conservative, and correct against the Upstream Cache's
  behaviour of rewriting the tree only when the Version changes. A manifest (toolchain
  identity + full flag vectors + source root) guards the object directory; any mismatch wipes
  it, so objects built with other flags or from another tree are never linked. Each of the
  four flag variants (Release/Debug × 43-Civs) keeps its own object directory.
- **`commit_id.inc` is generated** (upstream generates it with `git describe` as a VS
  pre-build step; two sources `#include` it). The installer runs no git, so the selected
  Version — `BuildRequest::version_label` — stands in, sanitised, suffixed "Installer".
  Written into the source root like upstream's own builds do (it is transient there: the
  Upstream Cache rewrites the tree on Version change), and only when its content changed, so
  it never dirties an incremental rebuild.
- **Failures**: every tool's full output goes to `build.log` in the object directory, in
  deterministic source order; the user gets one sentence (the Core appends the
  "try a Release" suggestion per rule 10), the log path and first error go to the log detail.
- **Debug configuration** is implemented and unit-tested (flag vector asserted literally) but
  no real Debug compile has been run; ticket 08's Dev mode is where it becomes reachable and
  should be exercised for real.
- Parallelism is `available_parallelism`, bounded workers over an index queue —
  the reference script spawns all 172 clang processes at once, which was not worth copying.

### Post-review correction: `commit_id.inc` no longer touches the Installation Source

`/code-review` (both axes, independently) flagged the original design — writing the generated
`commit_id.inc` into the source root — as a rule-6 / user-story-29 breach: a Local Repo is
"used as-is", and a file the installer writes into a developer's checkout is a mutation of it,
gitignored or not. Now the file is generated under the variant directory and reached through
one extra `/I` directory: MSVC-style quoted includes try the including file's directory first
(so a `commit_id.inc` a developer's tree already has still wins — exactly "as-is") and fall
back to each `/I` entry joined with the relative path, where
`<variant>/generated/include/../commit_id.inc` resolves to ours. Proven against the real
compiler: clean 78 s build with the checkout's root verified untouched afterwards.

Two smaller findings fixed in the same pass: `CONTEXT.md` gained the **Build Configuration**
term the Core now uses (rule 16), and `docs/spec.md`'s "`.civ5proj`" wording was corrected to
name the real DLL project file, `VoxPopuli.vcxproj` — the `.civ5proj` files are the mods'
ModBuddy projects and list no C++ sources. Judgement-call smells (the build-context parameter
clump in `DllBuild::compile`/`link`, the duplicated shim/stub compile shape) are noted and
left for a quieter refactor.
