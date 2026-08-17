//! Startup wiring only. Everything else lives in the library half of this crate.

use std::process::ExitCode;
use std::sync::Arc;

use civ5vp_core::{AppDataStore, SearchLocations};
use civ5vp_installer::cli::{self, Command};
use civ5vp_installer::{InstallerApp, screenshot, wiring};

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
    // The App Data Store is the installer's one directory: the Core works in it, and the
    // Upstream Cache, Toolchain Cache and settings all live inside it. The executable itself
    // stores nothing beside itself.
    let store = AppDataStore::for_this_platform().map_err(|problem| {
        eprintln!("[civ5vp-installer] {}", problem.log_detail());
        problem.user_message()
    })?;
    // From here on, everything `log_detail` receives is on disk as well as stderr.
    civ5vp_installer::init_log_file(store.root().join("installer.log"));
    let core = Arc::new(wiring::core(&store));
    let locations = SearchLocations::for_this_platform();
    // The update ping: fired once, in the background, entirely best-effort.
    // Offline or failing, nothing arrives and nothing is shown — launch never waits on it.
    let (newer_sender, newer_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(tag) = civ5vp_installer::update::check_for_newer_release() {
            let _ = newer_sender.send(tag);
        }
    });
    // The window icon, by the two routes desktops actually use. `with_icon` carries the logo
    // on Windows and on X11; Wayland has no protocol for a client to set its own icon, so
    // there the compositor matches `app_id` against an installed desktop entry —
    // `packaging/civ5vp-installer.desktop`, whose file name this must keep matching.
    let mut viewport = egui::ViewportBuilder::default().with_app_id("civ5vp-installer");
    if let Some(icon) = civ5vp_installer::theme::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let native_options = eframe::NativeOptions {
        viewport: viewport
            .with_title("Civ 5 VP Installer")
            .with_inner_size(cli::DEFAULT_SIZE)
            // The layout is designed down to this size and no further; below it the
            // activity log would have no room left.
            .with_min_inner_size(cli::DEFAULT_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "civ5vp-installer",
        native_options,
        Box::new(move |_cc| {
            Ok(Box::new(
                InstallerApp::launch(core, store, &locations).with_update_check(newer_receiver),
            ))
        }),
    )
    .map_err(|err| format!("could not open the installer window: {err}"))
}
