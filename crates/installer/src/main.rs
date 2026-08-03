//! Startup wiring only. Everything else lives in the library half of this crate.

use std::process::ExitCode;
use std::sync::Arc;

use civ5vp_installer::cli::{self, Command};
use civ5vp_installer::{InstallerApp, placeholder, screenshot};

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(problem) => {
            eprintln!("{problem}\n\n{}", cli::USAGE);
            return ExitCode::FAILURE;
        }
    };

    let outcome = match command {
        Command::Help => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Command::Screenshot(options) => screenshot::run(&options),
        Command::RunApp => run_app(),
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("{problem}");
            ExitCode::FAILURE
        }
    }
}

fn run_app() -> Result<(), String> {
    let core = Arc::new(placeholder::core(work_dir()));
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Civ 5 VP Installer")
            .with_inner_size(cli::DEFAULT_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "civ5vp-installer",
        native_options,
        Box::new(|_cc| Ok(Box::new(InstallerApp::new(core)))),
    )
    .map_err(|err| format!("could not open the installer window: {err}"))
}

/// Scratch space for the Core. The App Data Store — the real home for this, alongside the
/// Upstream Cache and the Toolchain Cache — arrives with ticket 03.
fn work_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("civ5vp-installer")
}
