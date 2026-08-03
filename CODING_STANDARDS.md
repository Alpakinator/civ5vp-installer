# Coding Standards — Civ 5 VP Installer

The invariants that make this project's architecture hold. Each one traces to `docs/spec.md` or an ADR; the trace is given so a reviewer can check the rule against its source rather than take it on faith.

`/code-review`'s **Standards** axis reads this file. Violations are review findings, not style nits.

> **Status:** drafted from the spec before any code exists. Rules will need adjusting once ticket 01 lands and reality pushes back — amend this file rather than quietly working around it.

## Architecture

**1. The Core is headless and knows nothing about egui — enforced by the crate boundary.**
The Core is its own library crate, separate from the binary crate holding the egui shell. Its `Cargo.toml` does not list `egui`, `eframe`, or any windowing/graphics crate, so a UI type appearing in Core logic is a compile error rather than something a reviewer has to notice. If a Core type seems to need a colour or a rectangle, the design is wrong.
_Spec: "a headless Core is the single primary seam… the egui layer is a thin shell over the Core."_

**2. The Core has exactly two injected boundaries: the source provider and the toolchain runner.**
These are traits, injected at construction. Everything else — detection, extraction, fingerprinting, Sync — is concrete behind the Core. Adding a third injection point is an architectural change: raise it, don't just do it.
_Spec: "Inside the Core exactly two injected boundaries exist."_

**3. The egui shell contains no logic beyond calling the Core and rendering what it returns.**
No domain decisions in the UI layer: not which folders are Claimed, not whether EUI is legal with the chosen Flavor, not what order things happen in. The shell may own pure presentation state (which panel is open, scroll position). If a rule could be tested, it belongs in the Core.
_Spec + ticket 01: "the egui shell contains no logic beyond calling the Core."_

**4. Platform-specific code lives in thin adapters at the edge.**
`#[cfg(windows)]` / `#[cfg(unix)]` appears only in adapter modules (path detection, known folders, registry, case-folding). Core logic is platform-agnostic and tested on Linux. The thinner the adapter, the less untested Windows code exists.
_Spec: "development and routine verification happen on EndeavourOS (Arch) only… Windows-specific code paths must stay behind the tested Core seam with platform adapters as thin as possible."_

**5. No external process except what the installer itself brings.**
Assume the user's machine has **nothing**: no git, no Python, no Docker, no wine, no 7-Zip, no msitools, no compiler, no Rust. The executable is a lone file that works on a fresh machine with no admin rights (user story 34). Any `Command::new` naming a tool the user would have to install is a defect. ISO/MSI/CAB extraction and build orchestration happen in-process. The permitted exception is invoking the bootstrapped clang/lld from the Toolchain Cache, and that goes through the toolchain-runner boundary.
_ADR-0001; spec: "no Visual Studio, no Docker, no Python on the user's machine… ISO9660 + MSI + CAB parsing inside the installer."_

Git is the one open question the spec leaves deliberately open: the preference is an embedded library doing a blobless partial clone, but the spec names "shipping a static git" as an accepted fallback if library support proves unreliable. If that fallback is taken, the binary the installer executes must be one it ships or bootstraps itself — never the user's `git` — and the decision needs an ADR.
_Spec, Further Notes: "blobless partial clone support in embeddable git libraries (fallbacks: full clone, or shipping a static git)."_

## Safety of the user's game

**6. Nothing outside the Claimed Folders and Claimed Files is ever written, moved, or deleted.**
Every path the installer writes to must be derived from a Claimed Folder root or be a Claimed File. Code that constructs a destination path any other way is a defect regardless of what it does. The game's `cache` folder is the one additional path the installer may clear; `ModUserData` is never touched.

Both sets are closed and both live in `CONTEXT.md`. Adding a member is a change to the safety boundary, not an implementation detail: amend `CONTEXT.md` in the same commit, or the rule silently stops meaning what it says. Claimed Files exist because the Text Folder belongs to the game — the installer deploys one file into it and cannot replace the folder wholesale the way it does its own.

_Spec: "never touches anything else"; user stories 22, 23. `CONTEXT.md`'s Text Folder entry — "Receives `VPUI_tips_en_us.xml`… The third deployment target alongside the MODS and DLC Folders."_

**7. Strict ordering: fetch → build → Sync. Failure before Sync leaves the game untouched.**
No partial deployment. Nothing may be written into a Claimed Folder until the fetch and the build have both fully succeeded.
_Spec: "failure at any stage aborts before the game is touched"; user story 18._

**8. Sync is deterministic and idempotent.**
Running the same Install Configuration twice produces the same tree and the second run changes nothing. Never iterate a `HashMap`/`HashSet` when the order affects file operations or hashing — collect and sort. Same input, same Build Fingerprint, byte for byte.
_Spec: "deterministically, with stale files removed"; Build Fingerprint definition in `CONTEXT.md`._

## Errors and logging

**9. No `unwrap`, `expect`, panic, or `unreachable!` on any path reachable from the UI.**
Core returns typed errors. `unwrap`/`expect` are permitted in tests and in `main()`'s startup wiring only. A panic in a deployed installer means a user's game is in an unknown state.

**10. Every user-facing error carries a plain-language message.**
Raw compiler output, git errors, and IO errors go to the log file; the UI gets a sentence a non-programmer can act on. Build failures suggest picking a Release.
_User stories 19, 20._

**11. Anything a user might report goes to the log file.**
The log is the support channel — full detail, stable line shapes, no swallowed errors.

## Tests

**12. Tests exercise external behavior through the Core seam only.**
Given a fixture repository and temporary MODS/DLC/Text directories, run an Install Configuration and assert the resulting file tree. No test reaches into Core internals, asserts on private structs, or exists only to pin an implementation detail. Items are not made `pub` for testing. Because the Core is a separate crate (rule 1), integration tests under `tests/` can only see its public API — the boundary is enforced, not merely agreed.
_Spec: "No test reaches into Core internals; the egui shell is not unit-tested."_

**13. The two boundaries are faked in the fast suite.**
A fake source provider serves fixture trees; a fake toolchain runner emits a marker artifact as the Built DLL. The per-commit suite never downloads the 580 MB SDK and never invokes a real compiler.
_Spec: "This keeps the suite free of the 580 MB SDK download and multi-minute compiles."_

**14. Real-toolchain and real-clone integration tests are `#[ignore]`d, not deleted.**
They run on demand. "The tests pass" means the fast suite passed — say so explicitly, and never claim the real toolchain path works on the strength of fakes alone.
_Spec: "Real-toolchain and real-clone integration tests exist but run manually / on-demand."_

**15. UI work is verified by rendering, not by assertion of intent.**
Screens are covered by `egui_kittest` snapshot tests (AccessKit tree for behavior, rendered PNG for looks). "It should look right" is not a verification; a committed snapshot is. Snapshot baselines are reviewed before they are committed — an updated baseline that nobody looked at proves nothing.

## Vocabulary and dependencies

**16. Domain types and test names use `CONTEXT.md` terms exactly.**
`InstallConfiguration`, `ClaimedFolder`, `Sync`, `Deployment`, `BuiltDll`, `BuildFingerprint`, `UpstreamCache`, `ToolchainCache`, `LocalRepo`, `Flavor`, `Version`. Not synonyms, not abbreviations invented on the spot.

**17. Every new dependency needs a reason.**
The deliverable is one static binary per OS with no runtime dependencies beyond the desktop graphics stack. Prefer pure-Rust crates; be suspicious of anything pulling a C toolchain, and note the justification in the commit that adds it.
_ADR-0002._

**18. `unsafe` only in platform adapters, only with a safety comment.**
Realistically this means Windows API calls. Each block states why it is sound.

## Which of these are permanent, and which expire

Two different kinds of rule are mixed above, and they age differently.

**Project decisions (1–8, 10, 12–14, 16).** These encode choices about *this* installer that no model can infer: the shape of the seam, what Sync may touch, why Windows code hides behind an adapter, what "tested" means here. They are specification. They do not expire with model generations and they stay whether or not any agent would have got there alone. Their reader is `/code-review`, which needs a citable rule to turn a judgment call into a finding.

**Behaviour corrections (9, 11, 17, 18).** These are general engineering hygiene — don't `unwrap`, log enough to debug, justify dependencies, comment your `unsafe`. A capable model may well do all of this unprompted, in which case the rule is costing attention and buying nothing.

Treat the second group as **ablation candidates**. Once there is real code, the honest test is Anthropic's own: remove them, work normally, and watch whether the behaviour actually degrades. Add back only what gets violated repeatedly. Predicting which instructions a model needs is guesswork; observing which it ignores is not.

Re-run that check when a new model ships. The first group stays put.
