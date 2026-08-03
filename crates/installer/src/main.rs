//! Startup wiring only. Everything else lives in the library half of this crate.

use std::process::ExitCode;
use std::sync::Arc;

use civ5vp_core::{AppDataStore, SearchLocations};
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
    // The App Data Store is the installer's one directory: the Core works in it, the settings
    // live in it, and the Upstream Cache and Toolchain Cache join them there in later tickets.
    // The executable itself stores nothing beside itself.
    let store = AppDataStore::for_this_platform().map_err(|problem| {
        eprintln!("[civ5vp-installer] {}", problem.log_detail());
        problem.user_message()
    })?;
    let core = Arc::new(placeholder::core(store.root().to_path_buf()));
    let locations = SearchLocations::for_this_platform();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Civ 5 VP Installer")
            .with_inner_size(cli::DEFAULT_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "civ5vp-installer",
        native_options,
        Box::new(move |_cc| Ok(Box::new(InstallerApp::launch(core, store, &locations)))),
    )
    .map_err(|err| format!("could not open the installer window: {err}"))
}
