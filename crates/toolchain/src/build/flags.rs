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
//! apart, so `-external:I` appears once per directory - same directories, same compiler
//! search list, just not physically merged.

use std::path::{Path, PathBuf};

use civ5vp_core::{BuildConfiguration, FortyThreeCivs};

/// The Release optimisation flags, measured against the game rather than transcribed.
///
/// The reference build settled on `-Os /Ob0 /Oy-`, recording only "clang inlining crashes
/// Civ V". A campaign run against the real game (see `docs/dll-optimisation-flag-experiments.md`)
/// confirmed the crash and then found roughly 30% faster AI turns without touching its cause:
///
/// * `/Ob0` stays, and is the one flag that may never move. Every build with inlining crashes
///   the game while loading the mod - including `/Ob1`, which inlines only what the source
///   itself marked `inline` or `__forceinline`. The optimisation *level* is innocent.
/// * `/clang:-O3` is the single largest win, about 20% on AI turns. It has to be spelled
///   through the pass-through: `clang-cl` silently discards a bare `-O3` and builds at `-O0`,
///   with no warning. `-O3`'s gain here is its loop work, which needs no inlining.
/// * `/GS-` drops the stack-cookie check; `/clang:-fno-math-errno` lets dead math calls be
///   deleted without changing any result.
/// * `-march=x86-64-v2` replaces the reference's 2004-era SSE3 floor with SSE4.2 (2008).
///   Steam's July 2026 survey puts SSE3 at 98.02% and SSE4.2 at 97.88%, so this costs 0.14
///   points of compatibility. `x86-64-v3` measured no faster than `v2` and would have cost
///   nearly 3 points, so it is not taken.
/// * `/clang:-ffp-contract=off` is a correctness guard, not a speed flag, and must not be
///   removed while any `-march` above the baseline is in force. `clang-cl` defaults to
///   `-ffp-contract=on` at every level; on a CPU with FMA that fuses a multiply and an add
///   into one instruction, which rounds once instead of twice. Different float results mean
///   desynchronised multiplayer and diverging saves.
/// * The three `-mllvm` passes push loop unrolling and vectorisation past `-O3`'s defaults.
///
/// Frame pointers are deliberately absent: `/Oy-` is gone, which frees `EBP` as a general
/// register on 32-bit x86, where there are only eight.
pub const DEFAULT_RELEASE_OPTIMISATION: [&str; 12] = [
    "/clang:-O3",
    "/Ob0",
    "/GS-",
    "/clang:-fno-math-errno",
    "/clang:-ffp-contract=off",
    "-march=x86-64-v2",
    "-mllvm",
    "-unroll-threshold=300",
    "-mllvm",
    "-extra-vectorizer-passes",
    "-mllvm",
    "-enable-loopinterchange",
];

/// The Release link-time optimisation flags the reference build proved, and today's default.
///
/// `/OPT:REF` drops unreferenced code; `/OPT:ICF` folds functions with identical bodies into
/// one address - which is also why it is worth an experiment: folding breaks any code that
/// compares function pointers for identity.
pub const DEFAULT_RELEASE_LINK_OPTIMISATION: [&str; 2] = ["/OPT:REF", "/OPT:ICF"];

/// A maintainer's file, beside the installer executable, that replaces
/// [`DEFAULT_RELEASE_OPTIMISATION`] and [`DEFAULT_RELEASE_LINK_OPTIMISATION`] for one build.
///
/// This exists to answer a question the reference build left open: it settled on `-Os /Ob0`
/// because "clang inlining crashes Civ V", recording the symptom and not the cause, and
/// `/Ob0` costs every non-`__forceinline` call in the DLL. Finding a faster set that still
/// loads means building the same sources a dozen times with different flags, and the person
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
}

impl OptimisationFlags {
    /// Nothing to override on either side - the file said nothing at all.
    pub fn is_empty(&self) -> bool {
        self.compiler.is_empty() && self.linker.is_empty()
    }

    /// The compiler half in the shape [`compiler_args`] takes: `None` keeps the default.
    pub fn compiler_override(&self) -> Option<&[String]> {
        (!self.compiler.is_empty()).then_some(self.compiler.as_slice())
    }

    /// The linker half in the shape [`linker_args`] takes: `None` keeps the default.
    pub fn linker_override(&self) -> Option<&[String]> {
        (!self.linker.is_empty()).then_some(self.linker.as_slice())
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
        parts.join("; ")
    }
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
/// A line reading `[linker]` sends everything after it to lld-link, and `[compiler]` sends
/// it back to clang-cl. A file with no heading at all is entirely compiler flags, which is
/// what every file written before the linker half existed already meant.
pub fn parse_optimisation_override(contents: &str) -> OptimisationFlags {
    let mut flags = OptimisationFlags::default();
    let mut linker_section = false;
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line.to_ascii_lowercase().as_str() {
            "" => continue,
            "[linker]" => {
                linker_section = true;
                continue;
            }
            "[compiler]" => {
                linker_section = false;
                continue;
            }
            _ => {}
        }
        let half = if linker_section {
            &mut flags.linker
        } else {
            &mut flags.compiler
        };
        half.extend(line.split_whitespace().map(str::to_owned));
    }
    flags
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
pub fn compiler_args(
    configuration: BuildConfiguration,
    forty_three_civs: FortyThreeCivs,
    stackwalker: bool,
    source_root: &Path,
    sdk_include_dirs: &[std::path::PathBuf],
    optimisation_override: Option<&[String]>,
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
    for dir in sdk_include_dirs {
        args.push(format!("-external:I{}", dir.display()));
    }
    for suppress in CL_SUPPRESS {
        args.push(format!("-Wno-{suppress}"));
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
            "/clang:-O3",
            "/Ob0",
            "/GS-",
            "/clang:-fno-math-errno",
            "/clang:-ffp-contract=off",
            "-march=x86-64-v2",
            "-mllvm",
            "-unroll-threshold=300",
            "-mllvm",
            "-extra-vectorizer-passes",
            "-mllvm",
            "-enable-loopinterchange",
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
            "-external:I/sdk/VC/include",
            "-external:I/sdk/Include",
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
        );
        let default = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
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
        );
        let debug = compiler_args(
            BuildConfiguration::Debug,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
        );

        assert!(debug.contains(&"/Od".to_owned()));
        assert!(debug.contains(&"/DVPDEBUG".to_owned()));
        assert!(!debug.contains(&"/clang:-O3".to_owned()));
        assert!(!debug.contains(&"/Ob0".to_owned()));
        assert!(!debug.contains(&"-march=x86-64-v2".to_owned()));
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
        );
        let without = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
            None,
        );

        let position = with.iter().position(|a| a == "/DSTACKWALKER").unwrap();
        assert_eq!(with[position - 1], "/DEXTERNAL_PAUSING");
        assert_eq!(with[position + 1], "/DCVGAMECOREDLL_EXPORTS");
        assert!(!without.contains(&"/DSTACKWALKER".to_owned()));
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
        );
        for default in DEFAULT_RELEASE_OPTIMISATION {
            assert!(args.contains(&default.to_owned()), "{default} should remain");
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
