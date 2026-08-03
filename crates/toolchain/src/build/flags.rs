//! The exact clang-cl and lld-link settings that produce a DLL the game accepts.
//!
//! Transcribed from `build_vp_clang_linux.py` on the `docker` branch of
//! `Alpakinator/Community-Patch-DLL` — `build_cl_config_args` and `build_link_config_args`,
//! plus the constant tables they draw from. The spec is blunt about this: only that
//! configuration is proven, and Release settings taken from anywhere else yield a DLL the
//! game rejects. So this file *copies*; it does not derive, improve, or tidy. When comparing
//! against the Python, note that its `/I"…"` shell-quoting collapses to a single `"/I…"`
//! argument here, and its `'/D MOD_…'` string splits into the two arguments the shell made
//! of it.
//!
//! The one knowing deviation: the reference merges the VC9 CRT headers into a single SDK
//! `Include` directory at extraction time, so its flag builder takes one include root. Our
//! extraction honours the MSIs' real layout (ticket 05), which keeps the CRT and the SDK
//! apart, so `-external:I` appears once per directory — same directories, same compiler
//! search list, just not physically merged.

use std::path::Path;

use civ5vp_core::{BuildConfiguration, FortyThreeCivs};

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
/// orchestrator decides their order). `stackwalker` comes from the Version's project file —
/// see [`super::project::DllProject::stackwalker`]; when set, the define lands where
/// upstream's own clang scripts put it, right after `EXTERNAL_PAUSING`.
pub fn compiler_args(
    configuration: BuildConfiguration,
    forty_three_civs: FortyThreeCivs,
    stackwalker: bool,
    source_root: &Path,
    sdk_include_dirs: &[std::path::PathBuf],
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
        // -Os: best size/perf trade-off; /Ob0: clang inlining crashes (reference comment).
        BuildConfiguration::Release => args.extend(["-Os", "/Ob0", "/Oy-"].map(str::to_owned)),
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
pub fn linker_args(configuration: BuildConfiguration, source_root: &Path) -> Vec<String> {
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
        args.extend(["/OPT:REF", "/OPT:ICF"].map(str::to_owned));
    }
    args
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sdk_dirs() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/sdk/VC/include"),
            PathBuf::from("/sdk/Include"),
        ]
    }

    /// The Release flags, spelled out in full against the reference script — a transcription
    /// is only trustworthy if a reviewer can diff it without opening the Python.
    #[test]
    fn release_compiler_flags_match_the_reference_build() {
        let args = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
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
            "-Os",
            "/Ob0",
            "/Oy-",
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
        assert_eq!(args, expected);
    }

    #[test]
    fn debug_swaps_the_optimisation_flags_and_predefs() {
        let release = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
        );
        let debug = compiler_args(
            BuildConfiguration::Debug,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
        );

        assert!(debug.contains(&"/Od".to_owned()));
        assert!(debug.contains(&"/DVPDEBUG".to_owned()));
        assert!(!debug.contains(&"-Os".to_owned()));
        assert!(!debug.contains(&"/Ob0".to_owned()));
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
        );

        let d = args.iter().position(|a| a == "/D").unwrap();
        assert_eq!(args[d + 1], "MOD_GLOBAL_MAX_MAJOR_CIVS=43");
        assert_eq!(args[d - 1], "/Oy-");
        assert_eq!(args[d + 2], "/DFXS_IS_DLL");

        let without = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
        );
        assert!(
            !without
                .iter()
                .any(|a| a.contains("MOD_GLOBAL_MAX_MAJOR_CIVS"))
        );
    }

    /// `STACKWALKER` tracks the Version's project file, and lands where upstream's own clang
    /// scripts put it — directly after `EXTERNAL_PAUSING`.
    #[test]
    fn stackwalker_is_defined_only_when_the_project_file_says_so() {
        let with = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            true,
            Path::new("/src"),
            &sdk_dirs(),
        );
        let without = compiler_args(
            BuildConfiguration::Release,
            FortyThreeCivs::Disabled,
            false,
            Path::new("/src"),
            &sdk_dirs(),
        );

        let position = with.iter().position(|a| a == "/DSTACKWALKER").unwrap();
        assert_eq!(with[position - 1], "/DEXTERNAL_PAUSING");
        assert_eq!(with[position + 1], "/DCVGAMECOREDLL_EXPORTS");
        assert!(!without.contains(&"/DSTACKWALKER".to_owned()));
    }

    #[test]
    fn release_linker_flags_match_the_reference_build() {
        let args = linker_args(BuildConfiguration::Release, Path::new("/src"));

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
        assert_eq!(args, expected);
    }

    #[test]
    fn debug_linking_drops_only_the_opt_flags() {
        let args = linker_args(BuildConfiguration::Debug, Path::new("/src"));

        assert!(!args.contains(&"/OPT:REF".to_owned()));
        assert!(!args.contains(&"/OPT:ICF".to_owned()));
        assert!(args.contains(&"/DEBUG".to_owned()));
        assert_eq!(
            args.last().unwrap(),
            "/DEF:/src/CvGameCoreDLL_Expansion2/CvGameCoreDLL.def"
        );
    }
}
