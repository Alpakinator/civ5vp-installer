# RESOLVED: the inlining crash, and what came of it

Closed 2026-08-25. This file was a handoff to a fresh agent describing an unapplied fix. The
fix has been applied, tested in the game, and shipped into the defaults, so what follows is a
record rather than a task. The original is kept at `.scratch/HANDOFF.superseded.md`.

## What was wrong

Every build with any inlining crashed the game while loading the mod, which forced `/Ob0` into
`DEFAULT_RELEASE_OPTIMISATION` and cost every non-`__forceinline` call in the DLL.

The cause was in this repository, not in clang and not in Vox Populi. `flags.rs` passed the
VC9 include directories with `-external:I`, which places them *ahead* of clang's own resource
include directory. VC9's `vadefs.h` defines `_crt_va_start` on x86 as address arithmetic on
the last named parameter - valid only while the function is not inlined. Clang ships its own
`vadefs.h` precisely to override that with `__builtin_va_start`, after which LLVM understands
the construct and declines to inline such functions at all; that refusal is the protection.
Ahead of clang's directory, the override was never reached.

## What was done

`crates/toolchain/src/build/flags.rs` now passes those directories with `/imsvc`, which places
them behind clang's. Verified at the codegen level on clang 18.1.8 and 22.1.8, then end to end:
unmodified upstream source built with `/Ob2` loads and plays.

The defaults went from twelve measured flags at `/Ob0` to upstream's own configuration:

    compiler   /Ox /Ob2 -flto=thin
    linker     /OPT:REF /OPT:ICF /LTCG

Measured on a 43-civ save - an AI turn fell from 138 s to 81 s, and the whole load-and-turn
cycle from 194 s to 133 s. Those figures compare this installer against its own previous
output, from the same source, machine and save.

Against the official 5.4.5 release DLL, same save and same 43-civ variant, the AI phase is
about 11% faster - 92 s to 81.5 s, two passes each, ranges not overlapping. The source differs
by a single commit touching four per-player accessors off the hot path, so this is a
compilation difference rather than a code one. It is not attributed further: this build also
passes `/Zc:threadSafeInit-`, which upstream does not, and uses ThinLTO where upstream uses
full LTO.

## Two claims in the original that were wrong

- **`/OPT:NOICF` was never a prerequisite.** The original insisted on it in three places. The
  null-vtable fault at `006B9612` was a downstream symptom of `DllMain` failing, and it is
  absent with `/OPT:ICF` on. The shipping default links with `/OPT:REF /OPT:ICF`.
- **"Nothing has been proven end to end"** was true of the include fix and not of `/Ob2`
  itself, which had already been shown to work through the source-side branch.

## Where the live information is now

- `DEFAULT_RELEASE_OPTIMISATION` and `compiler_args` in `crates/toolchain/src/build/flags.rs` -
  the flags, and the reasoning, beside the code they govern
- `CHANGELOG.md`, 0.1.5 - the same in prose
- `.scratch/turn-timing.md` - every measurement, the method, and its 4% noise floor
- `target/release/dll-flags.txt` - runs to reproduce any of it
- `.scratch/dll-optimisation-findings.md` and `docs/dll-optimisation-flag-experiments.md` -
  the original campaign, each now carrying a box saying which of its conclusions did not survive

## The Vox Populi branch is not needed

`/home/sunny/chest/Coding/CPDLL-sprintf-fix`, branch `fix-sprintf-s-inlining`, three unpushed
commits: ~90 call sites given explicit buffer sizes and nine variadic forwarders marked
`__declspec(noinline)`. It works, and it is unnecessary - the include fix repairs the same
thing for the CRT, for Vox Populi's own code, and for anything written later. No pull request
was opened, because there is no defect upstream: their build passes the include directories
through `vcvars`, which sets `INCLUDE`, and clang orders those correctly by itself.
