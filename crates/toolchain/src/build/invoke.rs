//! Running the bootstrapped tools - the one place the build touches `std::process`.
//!
//! External processes are forbidden throughout the installer, with exactly one permitted
//! exception: the clang/lld the installer bootstrapped itself, driven through the
//! toolchain-runner boundary. This module is that exception's narrow waist. It is a seam
//! rather than a bare `Command::new` so the fast suite can drive the whole orchestration -
//! staleness, parallelism, logging, failure surfacing - against a fake that never starts a
//! process.

use std::path::PathBuf;
use std::process::Command;

/// One tool invocation: which binary, its arguments, and where to run it.
///
/// The working directory is always the source root, and sources are passed relative to it -
/// clang-cl mistakes bare absolute Unix paths for MSVC-style options, which is why the
/// reference script also compiles from the project directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub current_dir: PathBuf,
    /// Environment set for this invocation only, on top of what the process already has.
    ///
    /// Empty for the DLL build, which needs nothing. It exists for the LuaJIT build's host
    /// tools: those run under wine on Linux, and wine with no `WINEPREFIX` creates one in the
    /// user's home and opens a Mono installer dialog at them. Containing that is not optional.
    pub env: Vec<(String, String)>,
}

impl ToolCommand {
    /// An invocation that inherits the environment unchanged - every command but the LuaJIT
    /// host tools.
    pub fn new(program: PathBuf, args: Vec<String>, current_dir: PathBuf) -> Self {
        Self {
            program,
            args,
            current_dir,
            env: Vec::new(),
        }
    }
}

/// What running a tool produced: whether it succeeded, and everything it printed.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub success: bool,
    /// stdout then stderr, lossily decoded - this goes into the build log verbatim.
    pub output: String,
}

/// The seam: the orchestrator asks for invocations, something answers.
///
/// `Sync` because stale sources compile on parallel worker threads sharing one invoker.
pub trait ToolInvoker: Sync {
    /// Run to completion. `Err` is "could not run at all" (missing binary, spawn failure);
    /// a tool that ran and failed is `Ok` with `success: false` and its output.
    fn run(&self, command: &ToolCommand) -> Result<ToolOutput, String>;
}

/// The real one: spawns the process and captures everything.
pub struct ProcessInvoker;

impl ToolInvoker for ProcessInvoker {
    fn run(&self, command: &ToolCommand) -> Result<ToolOutput, String> {
        let result = Command::new(&command.program)
            .args(&command.args)
            .current_dir(&command.current_dir)
            .envs(command.env.iter().map(|(key, value)| (key, value)))
            .output()
            .map_err(|error| {
                format!(
                    "failed to run {} in {}: {error}",
                    command.program.display(),
                    command.current_dir.display()
                )
            })?;
        let mut output = String::from_utf8_lossy(&result.stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(&result.stderr));
        Ok(ToolOutput {
            success: result.status.success(),
            output,
        })
    }
}
