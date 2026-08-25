//! The exact clang-cl and lld-link settings that produce a DLL the game accepts.
//!
//! Transcribed from `build_vp_clang_linux.py` on the `docker` branch of
//! `Alpakinator/Community-Patch-DLL` - `build_cl_config_args` and `build_link_config_args`,
//! plus the constant tables they draw from. The spec is blunt about this: only that
//! configuration is proven, and Release settings taken from anywhere else yield a DLL the
//! game rejects. So this file *copies*; it does not derive, improve, or tidy. When comparing
//! against the Python, note that its `/I"…"` shell-quoting collapses to a single `"/I…"`
//! argument here, and its `'/D MOD_…'` string splits into the two arguments the shell made
//! of it.
//!
//! One exception, added in 0.1.4: the Release *optimisation* flags are now measured rather
//! than transcribed - see [`DEFAULT_RELEASE_OPTIMISATION`]. Everything else on this page is
//! still a copy, and the measurement was made by changing only those flags and holding the
//! rest of the page fixed, which is the property
//! `an_override_replaces_the_optimisation_flags_and_nothing_else` exists to keep true.
//!
//! The one knowing deviation: the reference merges the VC9 CRT headers into a single SDK
//! `Include` directory at extraction time, so its flag builder takes one include root. Our
//! extraction honours the MSIs' real layout, which keeps the CRT and the SDK
//! apart, so `/imsvc` appears once per directory - same directories, same compiler
//! search list, just not physically merged.
//!
//! The second knowing deviation, added in 0.1.5: those directories are passed with `/imsvc`
//! rather than `-external:I`. See [`compiler_args`] - it is the difference between a DLL
//! that survives inlining and one that does not.

use std::path::{Path, PathBuf};

use civ5vp_core::{BuildConfiguration, FortyThreeCivs};

/// The Release optimisation flags: upstream's, after measurement failed to beat them.
///
/// These are exactly what `build_vp_clang.py` compiles Release with. That is a reversal - 0.1.4
/// shipped a measured set of twelve flags instead - and the reasoning is worth keeping, because
/// the obvious reading of the campaign in `docs/dll-optimisation-flag-experiments.md` is now
/// wrong.
///
/// That campaign ran entirely at `/Ob0`, because every build with inlining crashed the game on
/// mod load. Under that constraint it found roughly 30% in `-O3`, `-march=x86-64-v2`, `/GS-`
/// and three `-mllvm` passes. The constraint turned out to be self-inflicted: the installer
/// passed the VC9 headers with `-external:I`, which placed them ahead of clang's own and so
/// bypassed the `vadefs.h` correction clang ships - see [`compiler_args`]. With `/imsvc` the
/// crash is gone and `/Ob2` works against unmodified upstream source.
///
/// Re-measured at `/Ob2` against a 43-civ save (`.scratch/turn-timing.md`):
///
/// * **Inlining is worth 27%** - 194 s of AI turns down to about 140 s. That dwarfs everything
///   the campaign found, and it is the only result in the table that clears the noise.
/// * **The twelve flags are worth nothing measurable on top of it.** `/Ox /Ob2` alone, this
///   set, and Axatin's shipped DLL all landed between 137 s and 142 s - and a single
///   configuration measured twice spanned that whole range on its own. The instrument cannot
///   tell them apart, and more runs of the same kind would not change that.
/// * `-O3`'s campaign win does not survive the change. At `/Ob0` its raised inlining threshold
///   had nothing to act on, so only its loop work showed. At `/Ob2` both halves apply, on a
///   workload that is branchy and cache-bound.
///
/// So the tie is broken on everything except speed, and upstream's set wins on all of it:
/// `-msse3` keeps the SSE3 floor (98.02% of Steam hardware against SSE4.2's 97.88%), there are
/// ten fewer flags to justify, three of which reached into LLVM internals that a compiler bump
/// could reinterpret - and every future Vox Populi change is tested against these, not ours.
///
/// Note how little is left here. The reference's other Release flags - `-msse3`, `/GS`,
/// `/fp:precise` - are already in the fixed base at [`compiler_args`], so this half only has to
/// carry what the base does not. `/Ox` is clang-cl's deprecated spelling of `/O2`; verified
/// with `-###`, both lower to plain `-O2`, and `-O3` is reachable only as `/clang:-O3` because
/// clang-cl discards a bare `-O3` and silently builds at `-O0`.
///
/// `/Ob2` is the flag that matters, and the one that must not go back to `/Ob0` while
/// `compiler_args` passes the headers with `/imsvc`.
///
/// `-flto=thin` is the second thing that matters, and the only measured win left after
/// inlining. `/Ob2` inlines within one `.cpp` file; this DLL has 157 of them, and an AI turn
/// runs in `CvTacticalAI` and `CvHomelandAI` while calling into `CvUnit`, `CvPlot`, `CvCity`
/// and `CvPlayer` - four separate units, the largest 51,000 lines. Every one of those calls
/// crossed a wall until LTO moved optimisation into the linker, where the whole DLL is visible
/// at once. Measured: AI turns 90 s to 81.5 s, and the two sets of passes do not overlap -
/// every non-LTO measurement was at or above 137 s total, every LTO one at or below 134 s.
///
/// It was ruled out once, wrongly. The campaign's Run A was upstream master's exact
/// configuration, `-flto` and `/LTCG` included, and it crashed on load - but `/Ob2` alone
/// crashed then too, for the `vadefs.h` reason [`compiler_args`] now fixes. LTO was never
/// implicated; it was merely present in a build that was doomed anyway.
///
/// `thin`, not full: full LTO across 157 units this size costs minutes and gigabytes at link
/// time for no measured gain over ThinLTO. Deliberately absent is `-fwhole-program-vtables`,
/// LTO's usual companion: the game's own executable calls into this DLL through vtables, so
/// the program is not whole, and that assumption breaking is the exact shape of the bug that
/// cost this project its inlining in the first place.
///
/// The cost is link time - a full build went from about 1:40-2:00 to 2:23, and it falls on
/// every rebuild, not just the first install, because the link runs even when one file
/// changed. Thirty seconds a build against 9% of every AI turn, forever.
pub const DEFAULT_RELEASE_OPTIMISATION: [&str; 3] = ["/Ox", "/Ob2", "-flto=thin"];

/// The Release link-time optimisation flags the reference build proved, and today's default.
///
/// `/OPT:REF` drops unreferenced code; `/OPT:ICF` folds functions with identical bodies into
/// one address - which is also why it is worth an experiment: folding breaks any code that
/// compares function pointers for identity.
///
/// `/LTCG` is upstream's spelling of "link-time code generation" and pairs with
/// `-flto=thin` in [`DEFAULT_RELEASE_OPTIMISATION`]. `lld-link` reads the bitcode and does
/// LTO whether or not it is written, so this flag changes nothing on its own. It is kept for
/// two reasons that are not about behaviour: it is what upstream's `build_vp_clang.py` links
/// with, and it is what the measured configuration contained. Shipping something other than
/// what was measured, however inert, is not worth the saving of one word.
pub const DEFAULT_RELEASE_LINK_OPTIMISATION: [&str; 3] = ["/OPT:REF", "/OPT:ICF", "/LTCG"];

/// A maintainer's file, beside the installer executable, that replaces
/// [`DEFAULT_RELEASE_OPTIMISATION`] and [`DEFAULT_RELEASE_LINK_OPTIMISATION`] for one build.
///
/// It exists because the only way to answer a question about compiler flags here is to build
/// the same sources a dozen times and play the game after each one. That is how "clang
/// inlining crashes Civ V" was traced to an include-order mistake in this file rather than to
/// inlining, and how the flag set that finding made possible was then measured. The person
/// doing that runs the installer by double-clicking it. So the knob is a text file rather
/// than a command line, and it is deliberately not surfaced in the interface: players get one
/// default, chosen once it is proven.
pub const OPTIMISATION_OVERRIDE_FILE: &str = "dll-flags.txt";

/// The optimisation flags one run of the experiment uses, on either side of the build.
///
/// The two halves are separate because they reach different tools: a `/OPT:ICF` handed to
/// clang-cl is a source file it cannot open, and a `-mllvm` handed to lld-link is the same.
/// An empty half means "keep the proven default for that tool", so a file that names only
/// compiler flags still links the way the reference build links.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OptimisationFlags {
    /// Replaces [`DEFAULT_RELEASE_OPTIMISATION`] in the clang-cl command line.
    pub compiler: Vec<String>,
    /// Replaces [`DEFAULT_RELEASE_LINK_OPTIMISATION`] in the lld-link command line.
    pub linker: Vec<String>,
    /// Appended to the clang-cl command line *after* the `/D` predefs, replacing nothing.
    ///
    /// The other two halves land where the reference script puts optimisation flags, which is
    /// before [`SHARED_PREDEFS`] and [`RELEASE_ONLY_PREDEFS`]. clang-cl resolves `/D` and `/U`
    /// last-wins, so an override written there cannot switch a predef off - the `/D` that
    /// follows it wins. Measured on the pinned clang 18.1.8: `/DFOO /UFOO` leaves `FOO`
    /// undefined, `/UFOO /DFOO` leaves it defined.
    ///
    /// Without this half the file silently lied: a `/U` written under `[compiler]` was
    /// accepted, echoed back in the build summary, and then undone by the `/D` that followed
    /// it. It was built to test `STRONG_ASSUMPTIONS` - Release-only, mapping `ASSUME(x)` to
    /// `__builtin_assume(x)` and `UNREACHABLE_UNCHECKED()` to `__builtin_unreachable()` -
    /// which was the standing suspect for the inlining crash. It was innocent; the cause was
    /// the include order in [`compiler_args`]. The gap in the override file was real, and
    /// this closes it.
    pub after_predefs: Vec<String>,
}

impl OptimisationFlags {
    /// Nothing to override on either side - the file said nothing at all.
    pub fn is_empty(&self) -> bool {
        self.compiler.is_empty() && self.linker.is_empty() && self.after_predefs.is_empty()
    }

    /// The compiler half in the shape [`compiler_args`] takes: `None` keeps the default.
    pub fn compiler_override(&self) -> Option<&[String]> {
        (!self.compiler.is_empty()).then_some(self.compiler.as_slice())
    }

    /// The linker half in the shape [`linker_args`] takes: `None` keeps the default.
    pub fn linker_override(&self) -> Option<&[String]> {
        (!self.linker.is_empty()).then_some(self.linker.as_slice())
    }

    /// The after-the-predefs half. Empty is the ordinary case: this half adds, so there is no
    /// default for it to replace.
    pub fn after_predefs(&self) -> &[String] {
        &self.after_predefs
    }

    /// The whole set on one line, for the Activity panel - each half named, because
    /// "which tool did this flag reach" is the first thing a surprising result raises.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.compiler.is_empty() {
            parts.push(format!("compiler {}", self.compiler.join(" ")));
        }
        if !self.linker.is_empty() {
            parts.push(format!("linker {}", self.linker.join(" ")));
        }
        if !self.after_predefs.is_empty() {
            parts.push(format!("after predefs {}", self.after_predefs.join(" ")));
        }
        parts.join("; ")
    }
}

/// One sentence naming the optimisation flags this build will actually use, and where each
/// half came from.
///
/// Reported on every build, not only when an override is present. Silence used to mean "the
/// defaults", which is only legible to someone who already knows that - and the one question
/// a maintainer asks the Activity panel is *what did it just compile with*. Stating it
/// always also makes a file that was not picked up obvious: the line says `installer
/// default` where the maintainer expected their own flags.
///
/// The origins are tracked per half because an override may replace one and leave the other,
/// and a summary that blurred the two would be worse than none.
pub fn optimisation_summary(
    configuration: BuildConfiguration,
    over: Option<&OptimisationOverride>,
) -> String {
    if configuration == BuildConfiguration::Debug {
        return "Optimisation: none - this is a Debug build (compiler /Od /Oy-, no \
                link-time optimisation). dll-flags.txt does not apply to Debug builds."
            .to_owned();
    }
    const DEFAULT: &str = "installer default";
    let file = over.map(|o| o.source.display().to_string());
    let from_file = || file.clone().unwrap_or_else(|| DEFAULT.to_owned());

    let (compiler, compiler_from) = match over.and_then(|o| o.flags.compiler_override()) {
        Some(flags) => (flags.join(" "), from_file()),
        None => (DEFAULT_RELEASE_OPTIMISATION.join(" "), DEFAULT.to_owned()),
    };
    let (linker, linker_from) = match over.and_then(|o| o.flags.linker_override()) {
        Some(flags) => (flags.join(" "), from_file()),
        None => (
            DEFAULT_RELEASE_LINK_OPTIMISATION.join(" "),
            DEFAULT.to_owned(),
        ),
    };
    let mut summary = format!(
        "Optimisation: compiler {compiler} (from {compiler_from}); \
         linker {linker} (from {linker_from})"
    );
    // Only ever present when a file says so, so it needs no origin of its own - and it is
    // left out entirely when empty rather than shown as "none", which would read as a flag.
    if let Some(after) = over
        .map(|o| o.flags.after_predefs())
        .filter(|a| !a.is_empty())
    {
        summary.push_str(&format!("; after predefs {}", after.join(" ")));
    }
    summary
}

/// Release optimisation flags read from [`OPTIMISATION_OVERRIDE_FILE`], and where from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimisationOverride {
    pub flags: OptimisationFlags,
    pub source: PathBuf,
}

/// Read the override file if the maintainer has put one beside the installer executable.
///
/// Absent file, unreadable file, or a file holding nothing but blanks and comments all mean
/// "no override" - the default is what a player must get when nothing says otherwise.
pub fn read_optimisation_override() -> Option<OptimisationOverride> {
    read_optimisation_override_beside(std::env::current_exe().ok()?.parent()?)
}

/// [`read_optimisation_override`] against a named directory, so the file-finding half can be
/// tested without the test binary's own directory standing in for the installer's.
pub fn read_optimisation_override_beside(directory: &Path) -> Option<OptimisationOverride> {
    let path = directory.join(OPTIMISATION_OVERRIDE_FILE);
    let contents = std::fs::read_to_string(&path).ok()?;
    let flags = parse_optimisation_override(&contents);
    if flags.is_empty() {
        return None;
    }
    Some(OptimisationOverride {
        flags,
        source: path,
    })
}

/// Split the override file into flags: `#` starts a comment, and whitespace separates.
///
/// Flags are whitespace-separated rather than one-per-line because that is how they are
/// written everywhere else - in the reference script, in this file, and in the advice a
/// maintainer is copying from - and retyping them down a column invites transcription slips.
///
/// A line reading `[linker]` sends everything after it to lld-link, `[compiler]` sends it
/// back to clang-cl, and `[after-predefs]` sends it to clang-cl at the end of the command
/// line - see [`OptimisationFlags::after_predefs`] for why that position is its own section.
/// A file with no heading at all is entirely compiler flags, which is what every file written
/// before the other halves existed already meant.
pub fn parse_optimisation_override(contents: &str) -> OptimisationFlags {
    let mut flags = OptimisationFlags::default();
    let mut section = Section::Compiler;
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line.to_ascii_lowercase().as_str() {
            "" => continue,
            "[linker]" => {
                section = Section::Linker;
                continue;
            }
            "[compiler]" => {
                section = Section::Compiler;
                continue;
            }
            "[after-predefs]" => {
                section = Section::AfterPredefs;
                continue;
            }
            _ => {}
        }
        let half = match section {
            Section::Compiler => &mut flags.compiler,
            Section::Linker => &mut flags.linker,
            Section::AfterPredefs => &mut flags.after_predefs,
        };
        half.extend(line.split_whitespace().map(str::to_owned));
    }
    flags
}

/// Which half of [`OptimisationFlags`] the flags being read belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Compiler,
    Linker,
    AfterPredefs,
}

/// The DLL's base name. `civ5vp_core::BUILT_DLL_FILE_NAME` is this plus `.dll`.
pub const CORE_DLL: &str = "CvGameCore_Expansion2";

/// `DEF_FILE` in the reference script, relative to the source root.
pub const DEF_FILE: &str = "CvGameCoreDLL_Expansion2/CvGameCoreDLL.def";

/// The compatibility shim at the source root: MSVC intrinsics and CRT symbols clang emits
/// but the VC9 CRT does not carry. Compiled without the precompiled header.
pub const CLANG_SHIM: &str = "clang.cpp";

/// `PCH_CPP`: the one source compiled with `/Yc` to create the precompiled header.
pub const PCH_SOURCE: &str = "CvGameCoreDLL_Expansion2/_precompile.cpp";

/// `PCH_H`: the header every other source includes first and compiles through the PCH.
pub const PCH_HEADER: &str = "CvGameCoreDLLPCH.h";

/// `PCH`: the precompiled header artifact's file name.
pub const PCH_FILE: &str = "CvGameCoreDLLPCH.pch";

/// `INCLUDE_DIRS`: the project's own include directories, relative to the source root,
/// in the reference order.
pub const PROJECT_INCLUDE_DIRS: [&str; 8] = [
    "CvGameCoreDLL_Expansion2",
    "CvWorldBuilderMap/include",
    "CvGameCoreDLLUtil/include",
    "CvLocalization/include",
    "CvGameDatabase/include",
    "FirePlace/include",
    "FirePlace/include/FireWorks",
    "ThirdPartyLibs/Lua51/include",
];

/// `LIBS`: pre-built static libraries checked into the repository as COFF, linked in this
/// order, paths relative to the source root.
pub const PREBUILT_LIBS: [&str; 7] = [
    "CvWorldBuilderMap/lib/CvWorldBuilderMapWin32.obj",
    "CvGameCoreDLLUtil/lib/CvGameCoreDLLUtilWin32.lib",
    "CvLocalization/lib/CvLocalizationWin32.lib",
    "CvGameDatabase/lib/CvGameDatabaseWin32.lib",
    "FirePlace/lib/FireWorksWin32.obj",
    "FirePlace/lib/FLuaWin32.lib",
    "ThirdPartyLibs/Lua51/lib/lua51_Win32.lib",
];

/// `DEFAULT_LIBS`: system import libraries, resolved on the linker's library path.
pub const DEFAULT_LIBS: [&str; 15] = [
    "winmm.lib",
    "kernel32.lib",
    "user32.lib",
    "gdi32.lib",
    "winspool.lib",
    "comdlg32.lib",
    "advapi32.lib",
    "shell32.lib",
    "ole32.lib",
    "oleaut32.lib",
    "uuid.lib",
    "odbc32.lib",
    "odbccp32.lib",
    "msvcrt.lib",
    "oldnames.lib",
];

/// `SHARED_PREDEFS`, in the reference order.
const SHARED_PREDEFS: [&str; 9] = [
    "FXS_IS_DLL",
    "WIN32",
    "_WINDOWS",
    "_USRDLL",
    "EXTERNAL_PAUSING",
    "CVGAMECOREDLL_EXPORTS",
    "FINAL_RELEASE",
    "_CRT_SECURE_NO_WARNINGS",
    "_WINDLL",
];

/// What `RELEASE_PREDEFS` adds beyond the shared set.
const RELEASE_ONLY_PREDEFS: [&str; 3] = ["STRONG_ASSUMPTIONS", "NDEBUG", "VPRELEASE_ERRORMSG"];

/// What `DEBUG_PREDEFS` adds beyond the shared set.
const DEBUG_ONLY_PREDEFS: [&str; 1] = ["VPDEBUG"];

/// `CL_SUPPRESS`: warnings the reference build silences, "identical to Windows clang build".
const CL_SUPPRESS: [&str; 5] = [
    "invalid-offsetof",
    "tautological-constant-out-of-range-compare",
    "comment",
    "c++11-narrowing",
    "enum-constexpr-conversion",
];

/// `build_cl_config_args`: every compiler flag except the per-file `/Fo` / `/Yc` / `/Yu` /
/// `/Fp`, in the reference order.
///
/// `sdk_include_dirs` are the extracted SDK and VC9 CRT include roots (`crt_first` in the
/// orchestrator decides their order). `stackwalker` comes from the Version's project file -
/// see [`super::project::DllProject::stackwalker`]; when set, the define lands where
/// upstream's own clang scripts put it, right after `EXTERNAL_PAUSING`.
///
/// `optimisation_override` replaces the Release optimisation flags and nothing else - see
/// [`OPTIMISATION_OVERRIDE_FILE`]. It is ignored for Debug, which is not what anyone is
/// measuring, and taking a slice rather than reading the file here keeps this function a pure
/// transcription that a reviewer can diff against the Python.
///
/// `after_predefs` is appended last, after the `/D` predefs and the include directories, and
/// replaces nothing. It is the only place from which a `/U` can switch a predef off, because
/// clang-cl resolves the two last-wins. Also Release-only, on the same reasoning.
pub fn compiler_args(
    configuration: BuildConfiguration,
    forty_three_civs: FortyThreeCivs,
    stackwalker: bool,
    source_root: &Path,
    sdk_include_dirs: &[std::path::PathBuf],
    optimisation_override: Option<&[String]>,
    after_predefs: &[String],
) -> Vec<String> {
    let mut args: Vec<String> = [
        "-m32",
        "-msse3",
        "/c",
        "/MD",
        "/GS",
        "/EHsc",
        "/fp:precise",
        "/Zc:wchar_t",
        // Required, not preferred. `/Zc:threadSafeInit` makes clang emit calls to
        // `_Init_thread_header`, `_Init_thread_footer` and `_Init_thread_abort` around every
        // function-local static. Those three arrived with the VS2015 runtime; not one of them
        // exists in any library VC9 ships, and the game loads `msvcr90.dll`, so there is
        // nowhere else for them to come from. Turning this on fails at link with three
        // undefined symbols and twenty-odd references each - verified, 2026-08-25.
        //
        // It is also the one flag here that upstream's `build_vp_clang.py` does not pass,
        // which is unexplained: their build links against the same runtime. The likeliest
        // reading is that their clang emulates an older MSVC, for which clang-cl defaults
        // thread-safe statics off, but that has not been confirmed.
        "/Zc:threadSafeInit-",
        "/Zi",
    ]
    .map(str::to_owned)
    .to_vec();
    // /FS is Windows-only and omitted, exactly as the reference script notes.

    match configuration {
        BuildConfiguration::Release => match optimisation_override {
            Some(flags) => args.extend(flags.iter().cloned()),
            None => args.extend(DEFAULT_RELEASE_OPTIMISATION.map(str::to_owned)),
        },
        BuildConfiguration::Debug => args.extend(["/Od", "/Oy-"].map(str::to_owned)),
    }

    if forty_three_civs == FortyThreeCivs::Enabled {
        // The Python appends the single string '/D MOD_…=43'; its shell splits that into
        // these two arguments.
        args.push("/D".to_owned());
        args.push("MOD_GLOBAL_MAX_MAJOR_CIVS=43".to_owned());
    }

    for predef in SHARED_PREDEFS {
        args.push(format!("/D{predef}"));
        if stackwalker && predef == "EXTERNAL_PAUSING" {
            args.push("/DSTACKWALKER".to_owned());
        }
    }
    let configuration_predefs: &[&str] = match configuration {
        BuildConfiguration::Release => &RELEASE_ONLY_PREDEFS,
        BuildConfiguration::Debug => &DEBUG_ONLY_PREDEFS,
    };
    for predef in configuration_predefs {
        args.push(format!("/D{predef}"));
    }

    for dir in PROJECT_INCLUDE_DIRS {
        args.push(format!("/I{}", source_root.join(dir).display()));
    }
    // `/imsvc`, not `-external:I`, and the difference is the whole reason inlining works.
    //
    // Both flags mean "these are Microsoft's headers". They differ in where the directories
    // land in the search list: `-external:I` puts them *ahead* of clang's own resource
    // include directory, `/imsvc` puts them *behind* it.
    //
    // That matters because of one header. VC9's `vadefs.h` defines `_crt_va_start` on x86 as
    // address arithmetic on the last named parameter:
    //
    //     #define _crt_va_start(ap,v)  ( ap = (va_list)_ADDRESSOF(v) + _INTSIZEOF(v) )
    //
    // which is only true while the function still has a stack frame of its own - that is,
    // while it is *not* inlined. Clang ships its own `vadefs.h` precisely to override this:
    // it `#include_next`s VC9's, then redefines `_crt_va_start` to `__builtin_va_start`.
    // With the builtin, LLVM understands the construct and declines to inline the function,
    // and that refusal is the protection.
    //
    // Under `-external:I`, clang's copy is never reached, so every inlined variadic function
    // - the CRT's `sprintf_s` family and the mod's own `CvString::format` alike - reads its
    // arguments from the wrong address. Verified on clang 18.1.8 and 22.1.8 at `/Ob2`:
    // `-external:I` compiles the call with the `va_list` and the destination buffer at the
    // same address and the argument discarded; `/imsvc` emits a real out-of-line call. That
    // miscompile is what crashed the game inside `DllMain` on every build with inlining on.
    //
    // The cost: clang's copies of 18 headers now win over VC9's, `intrin.h` and the SSE
    // intrinsics family among them. That is the ordinary clang-cl arrangement, and clang's
    // intrinsics are the ones you want when clang is the compiler - but it is a real change
    // in surface, which is why this landed with a full DLL build and an in-game test rather
    // than on the strength of the unit suite.
    for dir in sdk_include_dirs {
        args.push(format!("/imsvc{}", dir.display()));
    }
    for suppress in CL_SUPPRESS {
        args.push(format!("-Wno-{suppress}"));
    }
    if configuration == BuildConfiguration::Release {
        args.extend(after_predefs.iter().cloned());
    }
    args
}

/// `build_link_config_args`: every linker flag except `/OUT`, `/PDB`, `/LIBPATH` and the
/// object list, in the reference order.
///
/// `optimisation_override` replaces [`DEFAULT_RELEASE_LINK_OPTIMISATION`] and nothing else,
/// on the same terms as the compiler half - see [`OPTIMISATION_OVERRIDE_FILE`]. Debug never
/// had those flags to begin with, so the override cannot reach it.
pub fn linker_args(
    configuration: BuildConfiguration,
    source_root: &Path,
    optimisation_override: Option<&[String]>,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "/MACHINE:x86",
        "/DLL",
        "/DEBUG",
        "/DYNAMICBASE",
        "/NXCOMPAT",
        "/SUBSYSTEM:WINDOWS",
        "/MANIFEST:EMBED",
        "/FORCE:MULTIPLE",
        "/NODEFAULTLIB:MSVCRT",
        "/NODEFAULTLIB:OLDNAMES",
        "/NODEFAULTLIB:VERSION",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(format!("/DEF:{}", source_root.join(DEF_FILE).display()));
    if configuration == BuildConfiguration::Release {
        match optimisation_override {
            Some(flags) => args.extend(flags.iter().cloned()),
            None => args.extend(DEFAULT_RELEASE_LINK_OPTIMISATION.map(str::to_owned)),
        }
    }
    args
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::PathBuf;

    /// The flags carry real paths, and `Path::join` uses the platform's separator - `\` on
    /// Windows, `/` everywhere else. Both are correct: clang-cl and lld-link accept either,
    /// and nothing downstream cares. Comparing against one written spelling therefore means
    /// normalising the separator, not asserting which platform the test runs on.
    fn with_forward_slashes(args: &[String]) -> Vec<String> {
        args.iter().map(|arg| arg.replace('\\', "/")).collect()
    }

    use super::*;

    fn sdk_dirs() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/sdk/VC/include"),
            PathBuf::from("/sdk/Include"),
        ]
    }

    /// The Release flags, spelled out in full. Everything outside the optimisation slot is a
    /// transcription and is only trustworthy if a reviewer can diff it against the Python
    /// without opening it; the optimisation flags themselves are measured, and are written
    /// out here so that changing them is never accidental.
    #[test]
    fn release_compiler_flags_match_the_reference_build() {
        let args = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );

        let expected: Vec<String> = [
            "-m32",
            "-msse3",
            "/c",
            "/MD",
            "/GS",
            "/EHsc",
            "/fp:precise",
            "/Zc:wchar_t",
            "/Zc:threadSafeInit-",
            "/Zi",
            "/Ox",
            "/Ob2",
            "-flto=thin",
            "/DFXS_IS_DLL",
            "/DWIN32",
            "/D_WINDOWS",
            "/D_USRDLL",
            "/DEXTERNAL_PAUSING",
            "/DCVGAMECOREDLL_EXPORTS",
            "/DFINAL_RELEASE",
            "/D_CRT_SECURE_NO_WARNINGS",
            "/D_WINDLL",
            "/DSTRONG_ASSUMPTIONS",
            "/DNDEBUG",
            "/DVPRELEASE_ERRORMSG",
            "/I/src/CvGameCoreDLL_Expansion2",
            "/I/src/CvWorldBuilderMap/include",
            "/I/src/CvGameCoreDLLUtil/include",
            "/I/src/CvLocalization/include",
            "/I/src/CvGameDatabase/include",
            "/I/src/FirePlace/include",
            "/I/src/FirePlace/include/FireWorks",
            "/I/src/ThirdPartyLibs/Lua51/include",
            "/imsvc/sdk/VC/include",
            "/imsvc/sdk/Include",
            "-Wno-invalid-offsetof",
            "-Wno-tautological-constant-out-of-range-compare",
            "-Wno-comment",
            "-Wno-c++11-narrowing",
            "-Wno-enum-constexpr-conversion",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(with_forward_slashes(&args), expected);
    }

    /// The override is a scalpel: the optimisation flags go, everything the reference build
    /// proved stays. A set that quietly dropped `/MD` or a predef would not be measuring the
    /// same DLL.
    #[test]
    fn an_override_replaces_the_optimisation_flags_and_nothing_else() {
        let overridden = vec!["-O2".to_owned(), "/Oy-".to_owned()];
        let args = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            Some(&overridden),
            &[],
        );
        let default = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );

        assert!(args.contains(&"-O2".to_owned()));
        for dropped in DEFAULT_RELEASE_OPTIMISATION {
            assert!(
                !args.contains(&dropped.to_owned()),
                "{dropped} should be gone: the override replaces the whole set"
            );
        }
        // Everything either side of the optimisation flags is untouched.
        let strip = |list: Vec<String>| -> Vec<String> {
            list.into_iter()
                .filter(|a| {
                    !DEFAULT_RELEASE_OPTIMISATION.contains(&a.as_str())
                        && !matches!(a.as_str(), "-O2" | "/Oy-")
                })
                .collect()
        };
        assert_eq!(strip(args), strip(default));
    }

    /// The half `read_optimisation_override_beside` cannot cover: that "beside the
    /// executable" really does mean the directory the running binary sits in. Nothing else
    /// proves the maintainer's file will be found at all, and a file silently not found
    /// yields a run of identical DLLs that read as "none of these flags did anything".
    #[test]
    fn the_override_is_read_from_the_running_executables_directory() {
        let beside = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        let path = beside.join(OPTIMISATION_OVERRIDE_FILE);
        assert!(
            !path.exists(),
            "{} already exists; this test would clobber it",
            path.display()
        );

        std::fs::write(&path, "-O2 /Oy-\n").unwrap();
        let found = read_optimisation_override();
        let runner_override = {
            use civ5vp_core::ToolchainRunner as _;
            crate::runner::BootstrappedToolchain::new(beside.join("unused-cache"))
                .dll_flag_override()
        };
        std::fs::remove_file(&path).unwrap();

        let found = found.expect("the file beside the executable should have been read");
        assert_eq!(found.flags.compiler, ["-O2", "/Oy-"]);
        assert!(found.flags.linker.is_empty());
        assert_eq!(found.source, path);

        // The wiring, asserted here rather than in its own test because the file beside the
        // running executable is a process-wide resource and two tests writing it would race.
        //
        // Worth the awkward placement: reading the file and *reporting* it to the Build
        // Fingerprint are separate steps, and when only the first one worked the installer
        // still built with the right flags - it just recorded "no override" beside the DLL
        // and skipped every later run. Nothing else fails when this is missing.
        assert_eq!(
            runner_override.as_deref(),
            Some("compiler -O2 /Oy-"),
            "the real runner must report the override to the Build Fingerprint"
        );
    }

    /// No file beside the installer is the case every player is in, and it must reach the
    /// default rather than an empty flag set.
    #[test]
    fn no_override_file_means_no_override() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_optimisation_override_beside(dir.path()), None);

        std::fs::write(
            dir.path().join(OPTIMISATION_OVERRIDE_FILE),
            "# run 1: baseline plus traps\n-Os /Ob0 /Oy- -fsanitize=undefined\n",
        )
        .unwrap();
        let found = read_optimisation_override_beside(dir.path()).unwrap();
        assert_eq!(
            found.flags.compiler,
            ["-Os", "/Ob0", "/Oy-", "-fsanitize=undefined"]
        );
        assert_eq!(found.source, dir.path().join("dll-flags.txt"));

        // A file emptied out - the obvious way to "turn it off" - is also no override.
        std::fs::write(
            dir.path().join(OPTIMISATION_OVERRIDE_FILE),
            "# off for now\n",
        )
        .unwrap();
        assert_eq!(read_optimisation_override_beside(dir.path()), None);
    }

    /// A maintainer copies a flag set out of a note and wants to keep the note. Comments and
    /// blank lines are therefore ordinary content, and a file holding only those is not an
    /// override at all - a player must get the default when nothing says otherwise.
    #[test]
    fn the_override_file_carries_comments_and_may_say_nothing() {
        assert_eq!(
            parse_optimisation_override(
                "# run 3: does inlining alone break it?\n-O2 /Ob2   /Oy-\n\n  /clang:-fno-unroll-loops # the likely fix\n"
            )
            .compiler,
            vec!["-O2", "/Ob2", "/Oy-", "/clang:-fno-unroll-loops"]
        );
        assert!(parse_optimisation_override("# nothing to see\n\n   \n").is_empty());
        assert!(parse_optimisation_override("").is_empty());
    }

    #[test]
    fn debug_swaps_the_optimisation_flags_and_predefs() {
        let release = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );
        let debug = compiler_args(
            BuildConfiguration::Debug,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );

        assert!(debug.contains(&"/Od".to_owned()));
        assert!(debug.contains(&"/DVPDEBUG".to_owned()));
        assert!(!debug.contains(&"/Ox".to_owned()));
        assert!(!debug.contains(&"/Ob2".to_owned()));
        assert!(!debug.contains(&"/DNDEBUG".to_owned()));
        assert!(!debug.contains(&"/DSTRONG_ASSUMPTIONS".to_owned()));
        assert!(!debug.contains(&"/DVPRELEASE_ERRORMSG".to_owned()));
        assert!(!release.contains(&"/DVPDEBUG".to_owned()));
    }

    /// The 43-Civs define arrives as the two arguments the reference build's shell produced,
    /// placed between the optimisation flags and the predefs.
    #[test]
    fn forty_three_civs_adds_the_define_in_the_reference_position() {
        let args = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Enabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );

        let d = args.iter().position(|a| a == "/D").unwrap();
        assert_eq!(args[d + 1], "MOD_GLOBAL_MAX_MAJOR_CIVS=43");
        assert_eq!(args[d - 1], *DEFAULT_RELEASE_OPTIMISATION.last().unwrap());
        assert_eq!(args[d + 2], "/DFXS_IS_DLL");

        let without = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );
        assert!(
            !without
                .iter()
                .any(|a| a.contains("MOD_GLOBAL_MAX_MAJOR_CIVS"))
        );
    }

    /// `STACKWALKER` tracks the Version's project file, and lands where upstream's own clang
    /// scripts put it - directly after `EXTERNAL_PAUSING`.
    #[test]
    fn stackwalker_is_defined_only_when_the_project_file_says_so() {
        let with = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            true,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );
        let without = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &[],
        );

        let position = with.iter().position(|a| a == "/DSTACKWALKER").unwrap();
        assert_eq!(with[position - 1], "/DEXTERNAL_PAUSING");
        assert_eq!(with[position + 1], "/DCVGAMECOREDLL_EXPORTS");
        assert!(!without.contains(&"/DSTACKWALKER".to_owned()));
    }

    /// The whole point of the third section: a `/U` written there must come *after* the
    /// matching `/D`, because clang-cl resolves the pair last-wins. Before this section
    /// existed the override landed among the optimisation flags, where the predefs that
    /// follow win and `STRONG_ASSUMPTIONS` could not be switched off at all.
    #[test]
    fn after_predefs_flags_land_after_every_predef() {
        let args = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Enabled,
            true,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &["/USTRONG_ASSUMPTIONS".to_owned()],
        );

        let undefine = args
            .iter()
            .position(|a| a == "/USTRONG_ASSUMPTIONS")
            .expect("the flag reached the command line");
        let define = args
            .iter()
            .position(|a| a == "/DSTRONG_ASSUMPTIONS")
            .expect("the predef is still emitted");
        assert!(
            undefine > define,
            "the /U must win: /D at {define}, /U at {undefine}"
        );
        // Last of all, so nothing the build appends later can be shadowed by it either.
        assert_eq!(undefine, args.len() - 1);
    }

    /// Debug never carries the Release predefs, so a section aimed at switching one off has
    /// nothing to act on there - and silently reaching Debug would make a maintainer's
    /// comparison build differ from the one upstream ships.
    #[test]
    fn after_predefs_flags_are_release_only() {
        let debug = compiler_args(
            BuildConfiguration::Debug,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
            &["/USTRONG_ASSUMPTIONS".to_owned()],
        );
        assert!(!debug.contains(&"/USTRONG_ASSUMPTIONS".to_owned()));
    }

    /// Three headings, each sending the rest of its lines to a different half.
    #[test]
    fn the_override_file_splits_into_three_halves() {
        let flags = parse_optimisation_override(
            "/clang:-O3 /Ob0\n\
             [linker]\n\
             /OPT:REF\n\
             [after-predefs]\n\
             /USTRONG_ASSUMPTIONS   # the experiment\n\
             [compiler]\n\
             /GS-\n",
        );

        assert_eq!(flags.compiler, ["/clang:-O3", "/Ob0", "/GS-"]);
        assert_eq!(flags.linker, ["/OPT:REF"]);
        assert_eq!(flags.after_predefs, ["/USTRONG_ASSUMPTIONS"]);
        assert_eq!(
            flags.summary(),
            "compiler /clang:-O3 /Ob0 /GS-; linker /OPT:REF; after predefs /USTRONG_ASSUMPTIONS"
        );
    }

    /// A file holding nothing but an `[after-predefs]` section is a real override - it must
    /// not read as "no file", or the build would skip the announcement and the fingerprint
    /// would record `flags none` while the build used them.
    #[test]
    fn an_after_predefs_only_file_is_not_empty() {
        let flags = parse_optimisation_override("[after-predefs]\n/USTRONG_ASSUMPTIONS\n");

        assert!(!flags.is_empty());
        assert!(flags.compiler_override().is_none());
        assert!(flags.linker_override().is_none());
        assert_eq!(flags.after_predefs(), ["/USTRONG_ASSUMPTIONS"]);
    }

    #[test]
    fn release_linker_flags_match_the_reference_build() {
        let args = linker_args(BuildConfiguration::Release, Path::new("/src"), None);

        let expected: Vec<String> = [
            "/MACHINE:x86",
            "/DLL",
            "/DEBUG",
            "/DYNAMICBASE",
            "/NXCOMPAT",
            "/SUBSYSTEM:WINDOWS",
            "/MANIFEST:EMBED",
            "/FORCE:MULTIPLE",
            "/NODEFAULTLIB:MSVCRT",
            "/NODEFAULTLIB:OLDNAMES",
            "/NODEFAULTLIB:VERSION",
            "/DEF:/src/CvGameCoreDLL_Expansion2/CvGameCoreDLL.def",
            "/OPT:REF",
            "/OPT:ICF",
            "/LTCG",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(with_forward_slashes(&args), expected);
    }

    #[test]
    fn debug_linking_drops_only_the_opt_flags() {
        let args = linker_args(BuildConfiguration::Debug, Path::new("/src"), None);

        assert!(!args.contains(&"/OPT:REF".to_owned()));
        assert!(!args.contains(&"/OPT:ICF".to_owned()));
        assert!(args.contains(&"/DEBUG".to_owned()));
        assert_eq!(
            with_forward_slashes(&args).last().unwrap(),
            "/DEF:/src/CvGameCoreDLL_Expansion2/CvGameCoreDLL.def"
        );
    }

    /// The two halves reach different tools, so a heading has to route them. Everything
    /// before any heading is compiler flags - which is what every file written before the
    /// linker half existed already meant, and those files must not change meaning.
    #[test]
    fn a_linker_heading_routes_the_flags_that_follow_it() {
        let parsed = parse_optimisation_override(
            "# run J\n/O2 /Ob2 /Oy-\n\n[linker]\n/OPT:REF   # keep folding off\n\n[compiler]\n-mllvm -inline-threshold=50\n",
        );

        assert_eq!(
            parsed.compiler,
            ["/O2", "/Ob2", "/Oy-", "-mllvm", "-inline-threshold=50"]
        );
        assert_eq!(parsed.linker, ["/OPT:REF"]);
        assert_eq!(parsed.compiler_override(), Some(parsed.compiler.as_slice()));
        assert_eq!(parsed.linker_override(), Some(parsed.linker.as_slice()));
    }

    /// A file that names only linker flags must still compile the way the reference build
    /// compiles - an empty half is "keep the default", not "use nothing".
    #[test]
    fn an_empty_half_keeps_that_tools_default() {
        let parsed = parse_optimisation_override("[linker]\n/OPT:REF\n");
        assert!(parsed.compiler.is_empty());
        assert_eq!(parsed.compiler_override(), None);
        assert!(!parsed.is_empty());

        let args = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            parsed.compiler_override(),
            &[],
        );
        for default in DEFAULT_RELEASE_OPTIMISATION {
            assert!(
                args.contains(&default.to_owned()),
                "{default} should remain"
            );
        }
    }

    /// The linker override is the same scalpel as the compiler one: the optimisation flags
    /// go, and every flag the reference build proved stays.
    #[test]
    fn a_linker_override_replaces_only_the_link_optimisation_flags() {
        let overridden = vec!["/OPT:REF".to_owned()];
        let args = linker_args(
            BuildConfiguration::Release,
            Path::new("/src"),
            Some(&overridden),
        );
        let default = linker_args(BuildConfiguration::Release, Path::new("/src"), None);

        assert!(args.contains(&"/OPT:REF".to_owned()));
        assert!(!args.contains(&"/OPT:ICF".to_owned()));
        let strip = |list: Vec<String>| -> Vec<String> {
            list.into_iter()
                .filter(|a| !DEFAULT_RELEASE_LINK_OPTIMISATION.contains(&a.as_str()))
                .collect()
        };
        assert_eq!(strip(args), strip(default));
    }

    /// Debug never carried the link optimisation flags, so an override has nothing to replace
    /// there - and must not smuggle Release's flags in.
    #[test]
    fn a_linker_override_cannot_reach_debug() {
        let overridden = vec!["/OPT:ICF".to_owned()];
        let args = linker_args(
            BuildConfiguration::Debug,
            Path::new("/src"),
            Some(&overridden),
        );
        assert!(!args.contains(&"/OPT:ICF".to_owned()));
        assert_eq!(
            args,
            linker_args(BuildConfiguration::Debug, Path::new("/src"), None)
        );
    }

    /// The Activity panel line has to say which tool each flag reached, because that is the
    /// first question a surprising result raises.
    /// The reported flags are what the build will really use, per half, with the origin of
    /// each - so a file that was not picked up reads as `installer default` where the
    /// maintainer expected their own flags.
    #[test]
    fn the_reported_optimisation_names_the_defaults_when_no_file_overrides() {
        let summary = optimisation_summary(BuildConfiguration::Release, None);

        assert!(
            summary.contains("compiler /Ox /Ob2 -flto=thin"),
            "{summary}"
        );
        assert!(
            summary.contains("linker /OPT:REF /OPT:ICF /LTCG"),
            "{summary}"
        );
        assert_eq!(
            summary.matches("from installer default").count(),
            2,
            "{summary}"
        );
        assert!(!summary.contains("after predefs"), "{summary}");
    }

    #[test]
    fn an_overridden_half_is_reported_against_its_file_and_the_other_against_the_default() {
        let over = OptimisationOverride {
            flags: parse_optimisation_override("/clang:-O3 /Ob2"),
            source: PathBuf::from("/beside/dll-flags.txt"),
        };

        let summary = optimisation_summary(BuildConfiguration::Release, Some(&over));

        assert!(
            summary.contains("compiler /clang:-O3 /Ob2 (from /beside/dll-flags.txt)"),
            "{summary}"
        );
        assert!(
            summary.contains("linker /OPT:REF /OPT:ICF /LTCG (from installer default)"),
            "{summary}"
        );
    }

    #[test]
    fn the_reported_optimisation_carries_the_after_predefs_half() {
        let over = OptimisationOverride {
            flags: parse_optimisation_override("[after-predefs]\n/USTRONG_ASSUMPTIONS"),
            source: PathBuf::from("/beside/dll-flags.txt"),
        };

        let summary = optimisation_summary(BuildConfiguration::Release, Some(&over));

        assert!(
            summary.ends_with("after predefs /USTRONG_ASSUMPTIONS"),
            "{summary}"
        );
        // The halves it did not speak for are still the installer's.
        assert!(
            summary.contains("compiler /Ox /Ob2 -flto=thin"),
            "{summary}"
        );
    }

    /// A Debug build takes no optimisation flags and ignores the file entirely, so the line
    /// must say that rather than name Release flags Debug will never use.
    #[test]
    fn a_debug_build_reports_that_the_override_does_not_reach_it() {
        let over = OptimisationOverride {
            flags: parse_optimisation_override("/clang:-O3 /Ob2"),
            source: PathBuf::from("/beside/dll-flags.txt"),
        };

        let summary = optimisation_summary(BuildConfiguration::Debug, Some(&over));

        assert!(summary.contains("Debug build"), "{summary}");
        assert!(summary.contains("does not apply"), "{summary}");
        assert!(!summary.contains("/Ob2"), "{summary}");
    }

    #[test]
    fn the_summary_names_each_half() {
        let parsed = parse_optimisation_override("/O2 /Ob2\n[linker]\n/OPT:REF\n");
        assert_eq!(parsed.summary(), "compiler /O2 /Ob2; linker /OPT:REF");
        assert_eq!(
            parse_optimisation_override("[linker]\n/OPT:REF\n").summary(),
            "linker /OPT:REF"
        );
    }
}
