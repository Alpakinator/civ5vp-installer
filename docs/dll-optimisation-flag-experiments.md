# Finding a faster DLL that still loads

A test campaign for the `dll-flags.txt` override (`crates/toolchain/src/build/flags.rs`). The goal: the most speed-optimised `CvGameCore_Expansion2.dll` that Civ 5 still loads.

> **The campaign has been run.** Its result is now the shipped default in
> `DEFAULT_RELEASE_OPTIMISATION` - roughly 30% faster AI turns, with inlining still off
> because inlining is what crashes the game. The rest of this page is the method and the
> traps, kept because the next question (why clang cannot inline this codebase at all) still
> needs them.

Everything below about compiler behaviour was checked against the compiler the installer actually ships - **clang 18.1.8**, `clang-cl` driver, `-m32`, the pinned build in `crates/toolchain/src/pinned.rs`. Flag behaviour differs between clang versions; do not carry these conclusions to another clang.

## 1. The two configurations that already exist

| | docker branch (what the installer uses) | upstream `master` `build_vp_clang.py` |
| --- | --- | --- |
| Optimisation | `-Os /Ob0 /Oy-` | `/Ox /Ob2 /Zo -flto` |
| Linker | `/OPT:REF /OPT:ICF` | `/OPT:REF /OPT:ICF /LTCG` |
| Also in the base flags | `/Zc:threadSafeInit-`, `-external:I` for the SDK | `/FS` (Windows-only) |

Both were fetched and diffed, not recalled. The docker branch's comment is `-Os: best size/perf; /Ob0: clang inlining crashes`.

Reduced to what actually changes code generation, `master` differs in exactly four ways:

1. **Optimisation level.** `/Ox` and `/O2` both reach cc1 as `-O2` (optimise for speed). `-Os` reaches it as `-Os` (optimise for size). Verified with `clang-cl -###`.
2. **Inlining.** `/Ob2` becomes `-finline-functions`; `/Ob0` becomes `-fno-inline`, which switches inlining off across the whole DLL - including calls the compiler would otherwise inline for free. This is the flag with the documented crash next to it and the one costing the most speed.
3. **Frame pointers.** `/Oy-` becomes `-mframe-pointer=all`. Without it, any optimisation level implies `-mframe-pointer=none`, so `master` omits frame pointers and the docker branch keeps them.
4. **Link-time optimisation.** `-flto` makes every `.obj` LLVM bitcode and moves optimisation into the linker, where it can inline across source files.

`/Zo` only affects debug information, not code.

## 2. Traps found in clang-cl 18.1.8

Read this section before writing a flag file. Three of these produce a build that looks like a successful experiment and measures nothing.

**Bare `-O3` and `-Oz` are silently ignored.** In `clang-cl` driver mode neither reaches cc1 at all, so the DLL is built at `-O0`. There is no warning. Verified: the same source compiled with `/Od`, `-O3` and `-Oz` produces byte-identical object sizes (661 bytes), while `/O2` gives 770 and `-Os` gives 649. The installer's `reject_ignored_flags` check cannot catch this, because it only looks for the compiler's `unknown argument` warning and no such warning is emitted.

`-O3` *is* reachable - spell it **`/clang:-O3`** and it arrives at cc1 as `-O3`, with the loop and SLP vectorisers on. Verified: `/clang:-O3` produces a 694-byte object against `/O2`'s 770 and `/Od`'s 661. `/clang:-Oz` likewise reaches cc1 as `-Oz`. **Never write `-O3` or `-Oz` bare; either use `/O2` / `/Ox`, or use the `/clang:` form.**

**Most `-f...` flags are rejected by the clang-cl driver.** `-fwrapv`, `-fno-vectorize`, `-fno-slp-vectorize`, `-fno-unroll-loops`, `-fno-inline-functions`, `-fno-omit-frame-pointer`, `-fno-jump-tables`, `-ffunction-sections`, `-fstack-protector-strong` and the `-f...-math-...` family all draw `unknown argument ignored in clang-cl`. The installer *will* refuse to build on these, which is the correct outcome - but write them as **`/clang:-fno-vectorize`** and they pass through to the compiler proper. All of the above were verified to be accepted in `/clang:` form.

`-fno-strict-aliasing`, `-fno-delete-null-pointer-checks`, `-fno-builtin`, `-mno-sse2`, `-march=`, `-mtune=` are accepted bare.

**Strict aliasing is already off, so `-fno-strict-aliasing` buys nothing.** `clang-cl` passes `-relaxed-aliasing` to cc1 at every optimisation level, deliberately matching MSVC, which never did type-based aliasing optimisation. Verified in the `-###` output. The usual first guess for "old C++ codebase miscompiles at high optimisation" is therefore already ruled out - and `/clang:-fstrict-aliasing` would turn the hazard *on*. Do not use it.

**Accepted but doing nothing:** `/Oi`, `/Ot`, `/GL`, `/GT`, `/Ob3`. Each parses without complaint and adds no cc1 flag beyond the level already selected. They are MSVC compatibility no-ops. Do not spend a run on them.

**`/Oy` is a no-op too**, because frame-pointer omission is already the default at every optimisation level. Only its negation, `/Oy-`, does anything.

**Vectorisation is already on in the current default.** Both `-Os` and `/O2` pass `-vectorize-loops -vectorize-slp`. Whatever distinguishes a loading DLL from a crashing one, it is not "the vectoriser got switched on".

**The linker is reachable through a heading.** A line reading `[linker]` sends everything after it to `lld-link` in place of `/OPT:REF /OPT:ICF`; `[compiler]` sends it back to clang-cl. A file with no heading is entirely compiler flags, so every file written before the heading existed still means what it meant. An empty half keeps that tool's proven default, so naming only linker flags still compiles the reference way.

Unlike clang-cl, `lld-link` cannot ignore a flag silently: an argument it does not recognise is treated as an input file and the link fails with `could not open '/BOGUSFLAG'`. That is why only the compiler half gets the `unknown argument` probe.

**Never change `/fp:precise`.** Floating point results feed multiplayer synchronisation and save games. `/fp:fast` would not crash; it would desync.

## 3. The runs

Each line is a complete `dll-flags.txt`. Put the file beside the installer executable, run a build, and confirm the Activity panel prints `Building with optimisation flags from ...` with the flags you wrote - if it does not, the file is in the wrong directory and you are re-testing the default. The flag set is part of the Build Fingerprint, so changing it invalidates the sidecar beside the deployed DLL and forces a real rebuild - including when you remove the file and go back to the default. Inside the build, changing the flags also changes the build manifest, so no objects are carried over from the previous run.

Test order matters: establish that your procedure can detect a crash before you trust it to report a success.

### Run A - negative control, expected to crash

```
# master's Release configuration, minus the linker's /LTCG
/Ox /Ob2 /Zo -flto
```

If this loads fine for you, your crash was never about optimisation flags and the rest of the campaign is measuring nothing. Stop and re-examine.

### Run B - baseline, expected to load

```
-Os /Ob0 /Oy-
```

The current default, written out explicitly. Time a long autoplay or a late-game turn here; it is the number everything else is compared against.

### Run C - speed without inlining

```
/O2 /Ob0 /Oy-
```

The single most promising change. `-O2` instead of `-Os` across the whole DLL while inlining - the documented suspect - stays off. If this loads, you have most of the win with none of the blamed transformation.

### Run D - the suspect, alone

```
-Os /Ob2 /Oy-
```

Inlining switched on with everything else exactly as the working baseline. This is the run that tests the docker branch's comment directly. If D crashes and C loads, inlining is confirmed as the cause and the level is innocent.

### Run E - between the two

```
/O2 /Ob1 /Oy-
```

`/Ob1` inlines only what the source marked `__inline` / `__forceinline`. The Civ 5 codebase marks its hot small accessors, so this can recover much of the inlining benefit while leaving the compiler's own judgement out of it.

### Run F - full speed, frame pointers kept

```
/O2 /Ob2 /Oy-
```

The target configuration. If this loads, you are done; anything beyond it is a small gain for a large risk.

### Run G - bisect the inliner

Only if D or F crashes. `/Ob0`'s `-fno-inline` is an all-or-nothing switch, but the inliner's cost threshold is continuous, so you can walk it:

```
/O2 /Oy- -mllvm -inline-threshold=50
```

Then 100, 225 (the default), 500. The two words `-mllvm` and `-inline-threshold=...` are separate flags; the file's whitespace splitting handles that correctly. Do not combine this with `/Ob0`, which disables inlining outright and makes the threshold meaningless.

### Run H - narrow a crash at full speed

Only if F crashes. Run these one at a time on top of F to find which transformation is responsible:

```
/O2 /Ob2 /Oy- /clang:-fno-vectorize /clang:-fno-slp-vectorize
```
```
/O2 /Ob2 /Oy- /clang:-fno-unroll-loops
```
```
/O2 /Ob2 /Oy- /clang:-fno-optimize-sibling-calls
```
```
/O2 /Ob2 /Oy- /clang:-fwrapv /clang:-fno-delete-null-pointer-checks
```

The last one keeps full optimisation but removes two licences the compiler takes from undefined behaviour - signed overflow wrapping and "this pointer was dereferenced, so it cannot be null". A 2008-era codebase has plenty of both, and unlike strict aliasing these are *not* already disabled in MSVC mode.

### Run I-bis - beyond `/O2`

Only after F loads. `/Ox` is not a higher level than `/O2` here - both are `-O2`. The step above them has to be spelled through the pass-through:

```
/clang:-O3 /Ob2 /Oy-
```

`-O3` over `-O2` mainly means more aggressive loop transformation and a higher inlining threshold, so if F already crashed this will too. Treat it as the last few percent, not as a place to start.

### Run I - frame pointers

```
/O2 /Ob2
```

Same as F without `/Oy-`. Only worth running once F is known to load. Omitting frame pointers frees a register on 32-bit x86, which is a real gain there, but it also breaks stack walking - so if the Version's project file sets `STACKWALKER`, expect worse crash reports in exchange.

### Run K - identical function folding

`/OPT:ICF` folds functions with identical bodies onto one address. Civ 5 stores and compares function pointers, and folding breaks pointer identity - a genuine crash suspect that no compiler flag can reach:

```
/O2 /Ob2 /Oy-
[linker]
/OPT:REF
```

### Run J - link-time optimisation

```
/O2 /Ob2 /Oy- -flto
```

Last, because it is the largest change and the hardest to reason about: it defers optimisation to the linker, and this build links against pre-built VC9-era COFF libraries and uses `/FORCE:MULTIPLE`, which lets duplicate symbols resolve arbitrarily. Untested here beyond confirming the flag is accepted and that `lld-link` handles bitcode. `/LTCG` can now be written under `[linker]` for fidelity with `master`, though `lld-link` treats it as a no-op and does LTO from the bitcode regardless.

## 4. Recording a result

For each run, note the flags, whether the mod loaded, and a repeatable timing - the same save advanced the same number of turns is far better than a feeling. Keep the note in the file itself; `#` starts a comment and the parser ignores it:

```
# Run C - 2026-08-21 - loaded fine, late-game turn 41s vs 58s baseline
/O2 /Ob0 /Oy-
```
