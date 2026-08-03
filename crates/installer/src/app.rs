//! The egui shell.
//!
//! Rule 3: nothing here decides anything. It collects paths, hands them to the Core, and
//! renders what comes back. Which folders are Claimed, whether a Flavor is legal, what order
//! things happen in — all of that lives behind [`Core`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use civ5vp_core::{
    Core, Flavor, FortyThreeCivs, GameFolders, InstallConfiguration, InstallError, InstallOutcome,
    InstallationSource, ProgressEvent, ProgressReporter,
};

use crate::placeholder;

/// What the shell is currently showing. Presentation state, not domain state, and
/// deliberately not public: nothing outside this crate should be reading the shell's mind.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Ready,
    Installing,
    Installed { summary: String },
    Failed { message: String },
}

/// The screens `--screenshot` renders and ticket 09 will style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Ready,
    Installing,
    Installed,
    Failed,
}

impl Screen {
    pub const ALL: [Self; 4] = [Self::Ready, Self::Installing, Self::Installed, Self::Failed];

    /// Used to name the PNG this screen renders to.
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Installing => "installing",
            Self::Installed => "installed",
            Self::Failed => "failed",
        }
    }
}

struct RunningInstall {
    progress: Receiver<ProgressEvent>,
    result: Receiver<Result<InstallOutcome, InstallError>>,
}

/// The whole installer UI.
pub struct InstallerApp {
    core: Arc<Core>,
    source_folder: String,
    mods_folder: String,
    dlc_folder: String,
    text_folder: String,
    activity: Vec<String>,
    status: Status,
    running: Option<RunningInstall>,
}

impl InstallerApp {
    pub fn new(core: Arc<Core>) -> Self {
        Self {
            core,
            source_folder: String::new(),
            mods_folder: String::new(),
            dlc_folder: String::new(),
            text_folder: String::new(),
            activity: Vec::new(),
            status: Status::Ready,
            running: None,
        }
    }

    /// Same, with the four paths already filled in.
    pub fn with_paths(
        core: Arc<Core>,
        source_folder: &std::path::Path,
        folders: &GameFolders,
    ) -> Self {
        Self {
            source_folder: display_path(source_folder),
            mods_folder: display_path(&folders.mods),
            dlc_folder: display_path(&folders.dlc),
            text_folder: display_path(&folders.text),
            ..Self::new(core)
        }
    }

    /// An app frozen in one screen, for `--screenshot` and for snapshot baselines. Nothing
    /// is installed from a preview — it only ever gets rendered.
    pub fn preview(screen: Screen) -> Self {
        let mut app = Self::new(Arc::new(placeholder::core(PathBuf::from(
            "/preview/app-data",
        ))));
        app.source_folder = "/home/player/src/Community-Patch-DLL".to_owned();
        app.mods_folder = "/home/player/…/Sid Meier's Civilization 5/MODS".to_owned();
        app.dlc_folder = "/home/player/…/Sid Meier's Civilization V/Assets/DLC".to_owned();
        app.text_folder = "/home/player/…/Sid Meier's Civilization 5/Text".to_owned();
        match screen {
            Screen::Ready => {}
            Screen::Installing => {
                app.status = Status::Installing;
                // Illustrative sample lines, not asserted against the Core's wording — a
                // preview never runs an install.
                app.activity = vec![
                    "Fetching sources: Getting the mod files ready.".to_owned(),
                    "Fetching sources: Mod files ready.".to_owned(),
                    "Building the DLL: Building the DLL with placeholder-toolchain-0.".to_owned(),
                ];
            }
            Screen::Installed => {
                app.status = Status::Installed {
                    summary: "Installed (1) Community Patch.".to_owned(),
                };
                app.activity = vec![
                    "Fetching sources: Mod files ready.".to_owned(),
                    "Building the DLL: DLL built.".to_owned(),
                    "Installing into the game: Installed (1) Community Patch.".to_owned(),
                ];
            }
            Screen::Failed => {
                app.status = Status::Failed {
                    message: "Could not download the mod files. Check your internet connection \
                              and try again."
                        .to_owned(),
                };
            }
        }
        app
    }

    /// The one line describing where the install has got to. Rendered as a label, which is
    /// also how the tests read it — there is no accessor for the shell's state, so the tests
    /// see exactly what a user (or a screen reader) sees.
    fn status_line(&self) -> String {
        match &self.status {
            Status::Ready => "Ready.".to_owned(),
            Status::Installing => "Installing…".to_owned(),
            Status::Installed { summary } => summary.clone(),
            Status::Failed { message } => message.clone(),
        }
    }

    /// Draw the whole UI. Shared by the real binary, `--screenshot`, and the kittest harness,
    /// so all three show the same pixels — which is what makes a snapshot baseline mean
    /// anything about the shipped window (rule 15).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll();
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            // Fill the window rather than shrink-wrapping the widgets, so the panel
            // background covers the whole surface.
            ui.set_min_size(ui.available_size());
            self.contents(ui);
        });
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        ui.heading("Civ 5 VP Installer");
        ui.label(
            "Walking skeleton: Community Patch only, from a local checkout, with a placeholder \
             DLL build. The installed DLL is a marker file, not a compiled one.",
        );
        ui.add_space(8.0);

        egui::Grid::new("folders")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for (label, value) in [
                    ("Community-Patch-DLL folder", &mut self.source_folder),
                    ("MODS folder", &mut self.mods_folder),
                    ("DLC folder", &mut self.dlc_folder),
                    ("Text folder", &mut self.text_folder),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).desired_width(360.0));
                    ui.end_row();
                }
            });

        ui.add_space(8.0);
        let busy = self.status == Status::Installing;
        if ui
            .add_enabled(!busy, egui::Button::new("Install"))
            .clicked()
        {
            self.start_install();
        }

        ui.add_space(8.0);
        ui.label(self.status_line());

        if !self.activity.is_empty() {
            ui.add_space(8.0);
            ui.group(|ui| {
                for line in &self.activity {
                    ui.label(line);
                }
            });
        }

        if self.running.is_some() {
            // The worker thread has no way to wake the UI, so keep painting while it runs.
            // Keyed on a real running install, not on the status, so a preview of the
            // installing screen still settles into a still frame for the renderer.
            ui.ctx().request_repaint();
        }
    }

    fn start_install(&mut self) {
        let configuration = InstallConfiguration {
            source: InstallationSource::LocalRepo {
                path: PathBuf::from(self.source_folder.trim()),
            },
            flavor: Flavor::CommunityPatch,
            forty_three_civs: FortyThreeCivs::Disabled,
        };
        let folders = GameFolders {
            mods: PathBuf::from(self.mods_folder.trim()),
            dlc: PathBuf::from(self.dlc_folder.trim()),
            text: PathBuf::from(self.text_folder.trim()),
        };

        let plan = match self.core.plan(&configuration, &folders) {
            Ok(plan) => plan,
            Err(error) => {
                crate::log_detail(&error.log_detail());
                self.status = Status::Failed {
                    message: error.user_message(),
                };
                return;
            }
        };

        let (progress_sender, progress) = channel();
        let (result_sender, result) = channel();
        let core = Arc::clone(&self.core);
        std::thread::spawn(move || {
            let reporter = ProgressReporter::to_channel(progress_sender);
            let _ = result_sender.send(core.execute(&plan, &reporter));
        });

        self.activity.clear();
        self.status = Status::Installing;
        self.running = Some(RunningInstall { progress, result });
    }

    /// Drain whatever the worker thread has produced since the last frame.
    fn poll(&mut self) {
        let Some(run) = self.running.take() else {
            return;
        };

        let mut lines = Vec::new();
        while let Ok(event) = run.progress.try_recv() {
            lines.push(format!("{}: {}", event.stage.label(), event.message));
        }

        let finished = match run.result.try_recv() {
            Ok(Ok(outcome)) => {
                let names: Vec<_> = outcome
                    .deployed
                    .iter()
                    .map(|folder| folder.folder_name())
                    .collect();
                self.status = Status::Installed {
                    summary: format!("Installed {}.", names.join(", ")),
                };
                true
            }
            Ok(Err(error)) => {
                crate::log_detail(&error.log_detail());
                self.status = Status::Failed {
                    message: error.user_message(),
                };
                true
            }
            Err(TryRecvError::Empty) => false,
            // The worker died without sending a result, so there is no Core error to quote —
            // this is the one message the shell has to author itself.
            Err(TryRecvError::Disconnected) => {
                self.status = Status::Failed {
                    message: "The install stopped unexpectedly. Your game has not been changed."
                        .to_owned(),
                };
                true
            }
        };

        if finished {
            // The worker reports its result last, but events it sent just before that may
            // still be sitting in the channel. Without this the tail of Sync — the lines
            // saying what was actually installed — would be dropped.
            while let Ok(event) = run.progress.try_recv() {
                lines.push(format!("{}: {}", event.stage.label(), event.message));
            }
        }

        self.activity.extend(lines);
        if !finished {
            self.running = Some(run);
        }
    }
}

fn display_path(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
