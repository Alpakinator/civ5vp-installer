//! Compiling LuaJIT into `lua51_Win32.dll`.
//!
//! This is `src/msvcbuild.bat`'s x86 dynamic-release path, expressed as a list of invocations
//! and driven through the same [`ToolInvoker`] seam the Built DLL uses. Following that script
//! rather than the Makefile is deliberate: the Makefile is written for a GCC-style driver, and
//! the toolchain the installer bootstraps is clang-cl and lld-link.
//!
//! The order below is not incidental and must not be rearranged:
//!
//! 1. `minilua` is built, because DynASM is a Lua program and nothing else can run it.
//! 2. DynASM turns `vm_x86.dasc` into `host/buildvm_arch.h`.
//! 3. `buildvm` is built - it *includes* the header step 2 wrote.
//! 4. `buildvm` emits the virtual machine as a PE object plus six generated headers.
//! 5. Only now can the library sources compile, because they include those headers.

use std::path::{Path, PathBuf};

use crate::build::invoke::ToolCommand;
use crate::error::ToolchainError;
use crate::luajit::host::HostRunner;

/// The file the game loads its Lua engine from. The name is not ours to choose.
pub const ENGINE_FILE_NAME: &str = "lua51_Win32.dll";

/// The rolling-release timestamp compiled into the version string.
///
/// `msvcbuild.bat` gets this by running `git show -s --format=%ct`. This crate does not start
/// external processes other than the compiler it bootstrapped, and it does not need to: the
/// LuaJIT commit is pinned (`docs/pinned-artifacts.md` §7), so its committer timestamp is a
/// constant. The engine reports itself as `LuaJIT 2.1.1785763465`.
pub const LUAJIT_RELVER: &str = "1785763465";

/// The library modules, in `msvcbuild.bat`'s order.
///
/// `buildvm` is handed this exact list six times and derives the bytecode, fast-function,
/// library and record definitions from it, so the order is part of the generated output.
const ALL_LIB: [&str; 12] = [
    "lib_base.c",
    "lib_math.c",
    "lib_bit.c",
    "lib_string.c",
    "lib_table.c",
    "lib_io.c",
    "lib_os.c",
    "lib_package.c",
    "lib_debug.c",
    "lib_jit.c",
    "lib_ffi.c",
    "lib_buffer.c",
];

/// DynASM's settings for a 32-bit Windows target.
///
/// No `P64` - that is the 64-bit flag, and the game is 32-bit.
const DASM_FLAGS: [&str; 11] = [
    "-LN",
    "-D",
    "WIN",
    "-D",
    "JIT",
    "-D",
    "FFI",
    "-D",
    "ENDIAN_LE",
    "-D",
    "FPU",
];

/// Everything the invocations need to know about the machine they run on.
pub struct LuaJitBuild {
    pub clang: PathBuf,
    pub lld_link: PathBuf,
    pub include_dirs: Vec<PathBuf>,
    pub lib_dirs: Vec<PathBuf>,
    pub host: HostRunner,
    /// Where wine may build its prefix. Unused when the host runs the tools natively.
    pub wine_prefix: PathBuf,
}

impl LuaJitBuild {
    /// Flags every compile shares.
    ///
    /// `/W0` because this is upstream's code, not ours: warnings from it are not actionable
    /// here and would only bury the errors that are.
    fn compile_flags(&self) -> Vec<String> {
        let mut flags = vec![
            "-m32".to_owned(),
            "--target=i386-pc-windows-msvc".to_owned(),
            "/nologo".to_owned(),
            "/c".to_owned(),
            "/O2".to_owned(),
            "/W0".to_owned(),
            "/D_CRT_SECURE_NO_DEPRECATE".to_owned(),
        ];
        for dir in &self.include_dirs {
            flags.push("-imsvc".to_owned());
            flags.push(dir.to_string_lossy().into_owned());
        }
        flags
    }

    fn lib_path_flags(&self) -> Vec<String> {
        self.lib_dirs
            .iter()
            .map(|dir| format!("/LIBPATH:{}", dir.display()))
            .collect()
    }

    fn compile(&self, src: &Path, args: Vec<String>) -> ToolCommand {
        ToolCommand::new(self.clang.clone(), args, src.to_path_buf())
    }

    fn link(&self, src: &Path, args: Vec<String>) -> ToolCommand {
        ToolCommand::new(self.lld_link.clone(), args, src.to_path_buf())
    }

    fn host_tool(&self, src: &Path, exe: &str, args: Vec<String>) -> ToolCommand {
        self.host
            .command(&src.join(exe), args, src, &self.wine_prefix)
    }

    /// The whole build, in order, with `src` as every invocation's working directory.
    ///
    /// `library_sources` is every `lj_*.c` and `lib_*.c` in `src` - passed in rather than read
    /// here so the ordering and flags can be asserted without a LuaJIT checkout on disk.
    pub fn commands(&self, src: &Path, library_sources: &[String]) -> Vec<ToolCommand> {
        let mut plan = Vec::new();
        let common = self.compile_flags();
        let libs = self.lib_path_flags();

        // 1. minilua - a cut-down Lua interpreter, needed only to run DynASM.
        let mut compile_minilua = common.clone();
        compile_minilua.push("host/minilua.c".to_owned());
        compile_minilua.push("/Fominilua.obj".to_owned());
        plan.push(self.compile(src, compile_minilua));

        let mut link_minilua = vec!["/nologo".to_owned(), "/MACHINE:X86".to_owned()];
        link_minilua.extend(libs.clone());
        link_minilua.push("/SUBSYSTEM:CONSOLE".to_owned());
        link_minilua.push("/OUT:minilua.exe".to_owned());
        link_minilua.push("minilua.obj".to_owned());
        plan.push(self.link(src, link_minilua));

        // 2. DynASM writes the interpreter's assembly as a C header buildvm then embeds.
        let mut dynasm = vec!["../dynasm/dynasm.lua".to_owned()];
        dynasm.extend(DASM_FLAGS.iter().map(|flag| (*flag).to_owned()));
        dynasm.push("-o".to_owned());
        dynasm.push("host/buildvm_arch.h".to_owned());
        dynasm.push("vm_x86.dasc".to_owned());
        plan.push(self.host_tool(src, "minilua.exe", dynasm));

        // 3. The version header, from the pinned release stamp written beside it.
        plan.push(self.host_tool(src, "minilua.exe", vec!["host/genversion.lua".to_owned()]));

        // 4. buildvm, which needs the header from step 2 and the DynASM sources.
        let mut compile_buildvm = common.clone();
        compile_buildvm.push("-I.".to_owned());
        compile_buildvm.push("-I../dynasm".to_owned());
        for source in [
            "host/buildvm.c",
            "host/buildvm_asm.c",
            "host/buildvm_fold.c",
            "host/buildvm_lib.c",
            // The PE object emitter. Without it the link fails on `_emit_peobj`, which is the
            // one step that makes this a Windows build at all.
            "host/buildvm_peobj.c",
        ] {
            compile_buildvm.push(source.to_owned());
        }
        plan.push(self.compile(src, compile_buildvm));

        let mut link_buildvm = vec!["/nologo".to_owned(), "/MACHINE:X86".to_owned()];
        link_buildvm.extend(libs.clone());
        link_buildvm.push("/SUBSYSTEM:CONSOLE".to_owned());
        link_buildvm.push("/OUT:buildvm.exe".to_owned());
        for object in [
            "buildvm.obj",
            "buildvm_asm.obj",
            "buildvm_fold.obj",
            "buildvm_lib.obj",
            "buildvm_peobj.obj",
        ] {
            link_buildvm.push(object.to_owned());
        }
        plan.push(self.link(src, link_buildvm));

        // 5. The generated half of LuaJIT: the VM itself as a PE object, then six headers the
        // library sources include.
        plan.push(self.host_tool(
            src,
            "buildvm.exe",
            vec![
                "-m".to_owned(),
                "peobj".to_owned(),
                "-o".to_owned(),
                "lj_vm.obj".to_owned(),
            ],
        ));
        for (mode, output) in [
            ("bcdef", "lj_bcdef.h"),
            ("ffdef", "lj_ffdef.h"),
            ("libdef", "lj_libdef.h"),
            ("recdef", "lj_recdef.h"),
            ("vmdef", "jit/vmdef.lua"),
        ] {
            let mut args = vec![
                "-m".to_owned(),
                mode.to_owned(),
                "-o".to_owned(),
                output.to_owned(),
            ];
            args.extend(ALL_LIB.iter().map(|lib| (*lib).to_owned()));
            plan.push(self.host_tool(src, "buildvm.exe", args));
        }
        plan.push(self.host_tool(
            src,
            "buildvm.exe",
            vec![
                "-m".to_owned(),
                "folddef".to_owned(),
                "-o".to_owned(),
                "lj_folddef.h".to_owned(),
                "lj_opt_fold.c".to_owned(),
            ],
        ));

        // 6. The library itself. `/MD` and `LUA_BUILD_AS_DLL` are what make this a DLL that
        // exports the Lua C API rather than a static library.
        let mut compile_library = common;
        compile_library.push("/arch:SSE2".to_owned());
        compile_library.push("/D_CRT_STDIO_INLINE=__declspec(dllexport)__inline".to_owned());
        compile_library.push("/DLUA_BUILD_AS_DLL".to_owned());
        compile_library.push("/MD".to_owned());
        compile_library.extend(library_sources.iter().cloned());
        plan.push(self.compile(src, compile_library));

        // 7. The link. The output name is the game's, because the game loads it by name.
        let mut link_dll = vec![
            "/nologo".to_owned(),
            "/DLL".to_owned(),
            "/MACHINE:X86".to_owned(),
            format!("/OUT:{ENGINE_FILE_NAME}"),
        ];
        link_dll.extend(libs);
        link_dll.push("/OPT:REF".to_owned());
        link_dll.push("/OPT:ICF".to_owned());
        link_dll.push("/INCREMENTAL:NO".to_owned());
        link_dll.extend(
            library_sources
                .iter()
                .map(|source| source.replace(".c", ".obj")),
        );
        // The interpreter core, which never had a `.c` of its own.
        link_dll.push("lj_vm.obj".to_owned());
        plan.push(self.link(src, link_dll));

        plan
    }
}

/// Every `lj_*.c` and `lib_*.c` in `src`, sorted.
///
/// Read from disk rather than hardcoded: the list changes between LuaJIT versions, and a
/// hardcoded one would fail as a link error about a missing symbol rather than as anything a
/// maintainer could act on. Sorted so a build is reproducible regardless of directory order.
pub fn library_sources(src: &Path) -> Result<Vec<String>, ToolchainError> {
    let entries = std::fs::read_dir(src).map_err(|error| {
        ToolchainError::new(
            "The LuaJIT source the installer downloaded is not complete.",
            format!("could not read {}: {error}", src.display()),
        )
    })?;
    let mut sources: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            (name.starts_with("lj_") || name.starts_with("lib_")) && name.ends_with(".c")
        })
        .collect();
    sources.sort();
    if sources.is_empty() {
        return Err(ToolchainError::new(
            "The LuaJIT source the installer downloaded is not complete.",
            format!("no lj_*.c or lib_*.c under {}", src.display()),
        ));
    }
    Ok(sources)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn a_build() -> LuaJitBuild {
        LuaJitBuild {
            clang: PathBuf::from("/llvm/bin/clang-cl"),
            lld_link: PathBuf::from("/llvm/bin/lld-link"),
            include_dirs: vec![PathBuf::from("/sdk/Include")],
            lib_dirs: vec![PathBuf::from("/sdk/Lib")],
            host: HostRunner::Native,
            wine_prefix: PathBuf::from("/cache/wineprefix"),
        }
    }

    fn plan() -> Vec<ToolCommand> {
        a_build().commands(
            Path::new("/lj/src"),
            &["lj_api.c".to_owned(), "lib_base.c".to_owned()],
        )
    }

    fn program_names(plan: &[ToolCommand]) -> Vec<String> {
        plan.iter()
            .map(|command| {
                command
                    .program
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// DynASM must run before buildvm compiles, because buildvm includes the header DynASM
    /// writes; and buildvm must run before the library compiles, because the library includes
    /// the six headers buildvm writes. Get this order wrong and the build fails deep in a C
    /// preprocessor error rather than anywhere informative.
    #[test]
    fn the_generators_run_before_the_code_that_includes_their_output() {
        let plan = plan();
        let dynasm = plan
            .iter()
            .position(|c| c.args.iter().any(|a| a.contains("dynasm.lua")))
            .expect("dynasm runs");
        let compile_buildvm = plan
            .iter()
            .position(|c| c.args.iter().any(|a| a == "host/buildvm.c"))
            .expect("buildvm is compiled");
        let run_buildvm = plan
            .iter()
            .position(|c| c.args.iter().any(|a| a == "peobj"))
            .expect("buildvm runs");
        let compile_library = plan
            .iter()
            .position(|c| c.args.iter().any(|a| a == "lj_api.c"))
            .expect("the library is compiled");

        assert!(dynasm < compile_buildvm, "{:?}", program_names(&plan));
        assert!(compile_buildvm < run_buildvm, "{:?}", program_names(&plan));
        assert!(run_buildvm < compile_library, "{:?}", program_names(&plan));
    }

    /// Civilization V is Lua 5.1 and so is Vox Populi. Lua 5.2 semantics would only add
    /// divergence from the engine the mods were written against.
    #[test]
    fn lua_5_2_compatibility_is_never_enabled() {
        for command in plan() {
            for argument in &command.args {
                assert!(
                    !argument.contains("LUA52COMPAT"),
                    "Lua 5.2 compatibility must stay off: {argument}"
                );
            }
        }
    }

    /// A 64-bit DLL would not load into a 32-bit game at all, and the file has to carry the
    /// name the game looks for.
    #[test]
    fn the_engine_is_a_32_bit_dll_under_the_name_the_game_loads() {
        let plan = plan();
        let link = plan.last().expect("a link step");
        assert!(link.args.iter().any(|a| a == "/MACHINE:X86"));
        assert!(link.args.iter().any(|a| a == "/DLL"));
        assert!(
            link.args
                .iter()
                .any(|a| a == &format!("/OUT:{ENGINE_FILE_NAME}"))
        );
        assert_eq!(ENGINE_FILE_NAME, "lua51_Win32.dll");
    }

    /// Every compile targets 32-bit x86, not just the link - a mismatch here shows up as an
    /// unreadable pile of link errors.
    #[test]
    fn every_compile_targets_32_bit_x86() {
        for command in plan() {
            if !command.program.ends_with("clang-cl") {
                continue;
            }
            assert!(
                command.args.iter().any(|a| a == "-m32"),
                "{:?}",
                command.args
            );
            assert!(
                command
                    .args
                    .iter()
                    .any(|a| a == "--target=i386-pc-windows-msvc"),
                "{:?}",
                command.args
            );
        }
    }

    /// The interpreter core has no `.c` file, so nothing derives it from the source list. If
    /// it is dropped the DLL links but every Lua call crashes the game.
    #[test]
    fn the_generated_vm_object_is_linked_in() {
        let plan = plan();
        let link = plan.last().expect("a link step");
        assert!(
            link.args.iter().any(|a| a == "lj_vm.obj"),
            "{:?}",
            link.args
        );
    }

    /// Whatever runs the host tools, the working directory is `src` - LuaJIT's build reads and
    /// writes relative paths throughout, so anywhere else silently produces nothing.
    #[test]
    fn every_invocation_runs_in_the_source_directory() {
        for command in plan() {
            assert_eq!(command.current_dir, PathBuf::from("/lj/src"));
        }
    }

    /// A directory with no LuaJIT in it is a broken download, and saying so beats a link error.
    #[test]
    fn an_empty_source_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let error = library_sources(dir.path()).expect_err("an empty directory is not LuaJIT");
        assert!(error.detail().contains("no lj_*.c"), "{}", error.detail());
    }

    /// Only LuaJIT's own translation units, and always in the same order.
    #[test]
    fn the_library_sources_are_the_lj_and_lib_files_sorted() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "lj_api.c",
            "lib_base.c",
            "lj_vm.h",
            "host_ignored.c",
            "ljamalg.c",
        ] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }

        let found = library_sources(dir.path()).expect("sources");
        assert_eq!(
            found,
            vec!["lib_base.c".to_owned(), "lj_api.c".to_owned()],
            "only lj_*.c and lib_*.c, sorted"
        );
    }
}
