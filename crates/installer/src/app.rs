//! The egui shell.
//!
//! Rule 3: nothing here decides anything. It collects paths, hands them to the Core, and
//! renders what comes back. Which folders are Claimed, whether a Flavor is legal, what order
//! things happen in, whether a folder really is the game, where the MODS Folder lives inside
//! it, what to tell a player whose game cannot be used — all of that lives behind [`Core`] and
//! the Core's detection functions. Every sentence this file puts on screen either came out of
//! the Core or is a fixed label.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use civ5vp_core::{
    AppDataStore, Core, Eui, Flavor, FolderRejected, FortyThreeCivs, GameFolders,
    InstallConfiguration, InstallError, InstallOutcome, InstallationSource, ProgressEvent,
    ProgressReporter, SearchLocations, Settings, resolve_game_folders, start_up,
};

use crate::{deco, placeholder, theme};

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
    FoldersNeeded,
    Ready,
    Installing,
    Installed,
    Failed,
}

impl Screen {
    pub const ALL: [Self; 5] = [
        Self::FoldersNeeded,
        Self::Ready,
        Self::Installing,
        Self::Installed,
        Self::Failed,
    ];

    /// Used to name the PNG this screen renders to.
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::FoldersNeeded => "folders-needed",
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
    store: AppDataStore,
    source_folder: String,
    game_folder: String,
    documents_folder: String,
    /// The Core's answer about the two folders above: the three Deployment targets it works
    /// out from them, or the sentence explaining why there are none. Recomputed whenever a
    /// folder is edited. The shell holds the answer; it never reaches one.
    resolved: Result<GameFolders, String>,
    /// The Installation Source that was remembered, kept as-is until the player names a folder
    /// of their own.
    ///
    /// This build has no Version picker, so the shell cannot draw an Upstream Cache selection
    /// — but it must not destroy one either. Synthesising a Local Repo from an empty text field
    /// and saving that would throw away a remembered Version the next release could still show.
    remembered_source: InstallationSource,
    flavor: Flavor,
    forty_three_civs: FortyThreeCivs,
    activity: Vec<String>,
    status: Status,
    running: Option<RunningInstall>,
    /// Whether the art-deco theme has been installed on the context yet. Fonts and style
    /// are set once per context, not per frame — rebuilding the font atlas is not free.
    skinned: bool,
}

/// The Flavor choices, as the player reads them.
///
/// There are three, not two-plus-a-checkbox, and that is the point: EUI is legal only with Vox
/// Populi, and listing the legal combinations means the shell cannot offer the illegal one
/// without a rule of its own to enforce (rule 3). [`Flavor`] makes the same guarantee at the
/// type level; this is that guarantee drawn.
fn flavor_choices() -> [(Flavor, &'static str); 3] {
    [
        (Flavor::CommunityPatch, "Community Patch only"),
        (
            Flavor::VoxPopuli { eui: Eui::Disabled },
            "Vox Populi — the full overhaul",
        ),
        (
            Flavor::VoxPopuli { eui: Eui::Enabled },
            "Vox Populi with EUI — adds the Enhanced User Interface",
        ),
    ]
}

impl InstallerApp {
    /// A launch: the Core reconciles what was remembered in the App Data Store with what it
    /// can detect, and the shell renders the answer.
    pub fn launch(core: Arc<Core>, store: AppDataStore, locations: &SearchLocations) -> Self {
        let startup = start_up(&store, locations);
        for line in &startup.log {
            crate::log_detail(line);
        }

        let source_folder = match &startup.configuration {
            Some(InstallConfiguration {
                source: InstallationSource::LocalRepo { path },
                ..
            }) => display_path(path),
            _ => String::new(),
        };
        // The remembered Flavor and toggles, or what the Core suggests to a new player.
        let (flavor, forty_three_civs) = match &startup.configuration {
            Some(configuration) => (configuration.flavor.clone(), configuration.forty_three_civs),
            None => (Flavor::suggested(), FortyThreeCivs::Disabled),
        };
        let game_folder = startup
            .game_installation
            .as_deref()
            .map_or_else(String::new, display_path);
        let documents_folder = startup
            .documents_folder
            .as_deref()
            .map_or_else(String::new, display_path);

        // Start-up may have something more specific to say than a field-by-field rejection —
        // that the game is the native Aspyr port, say. Which of the two a player sees is the
        // Core's judgement, made by `Startup::explanation`, not the shell's.
        let resolved = resolve(&game_folder, &documents_folder)
            .map_err(|rejected| startup.explanation(&rejected));

        let app = Self {
            core,
            store,
            remembered_source: startup
                .configuration
                .as_ref()
                .map_or_else(InstallationSource::unchosen, |c| c.source.clone()),
            source_folder,
            game_folder,
            documents_folder,
            resolved,
            flavor,
            forty_three_civs,
            activity: Vec::new(),
            status: Status::Ready,
            running: None,
            skinned: false,
        };
        // Detected folders are worth remembering too: the next launch then starts from them
        // without searching (user story 26).
        if app.resolved.is_ok() {
            app.remember();
        }
        app
    }

    /// An app frozen in one screen, for `--screenshot` and for snapshot baselines. Nothing is
    /// installed from a preview and nothing is remembered — it only ever gets rendered.
    pub fn preview(screen: Screen) -> Self {
        let store = AppDataStore::at(PathBuf::from("/preview/app-data"));
        let core = Arc::new(placeholder::core(store.root().to_path_buf()));
        let game = "/home/player/…/Sid Meier's Civilization V";
        let documents = "/home/player/…/Sid Meier's Civilization 5";
        let mut app = Self {
            core,
            store,
            remembered_source: InstallationSource::unchosen(),
            source_folder: "/home/player/src/Community-Patch-DLL".to_owned(),
            game_folder: game.to_owned(),
            documents_folder: documents.to_owned(),
            // The illustrative paths above are not on this machine, so the Core would reject
            // them. A preview states what it wants drawn rather than asking: it is a picture,
            // not a session.
            resolved: Ok(GameFolders {
                mods: PathBuf::from(format!("{documents}/MODS")),
                dlc: PathBuf::from(format!("{game}/Assets/DLC")),
                text: PathBuf::from(format!("{documents}/Text")),
            }),
            flavor: Flavor::suggested(),
            forty_three_civs: FortyThreeCivs::Disabled,
            activity: Vec::new(),
            status: Status::Ready,
            running: None,
            skinned: false,
        };
        match screen {
            Screen::Ready => {}
            // The native Aspyr port: the refusal a player is most likely to meet and least
            // likely to work out unaided (user story 14). The wording is the Core's, quoted.
            Screen::FoldersNeeded => {
                app.documents_folder = String::new();
                app.resolved = Err(format!(
                    "{game} is the native Linux version of Civilization V from Aspyr. Vox \
                     Populi needs the Windows version running under Proton and cannot be \
                     installed into the native port. In Steam, open the game's properties, set \
                     a Proton compatibility tool, run the game once, then try again."
                ));
            }
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
                // The Flavor above is Vox Populi with EUI, so this says what that installs —
                // a preview whose result contradicts its own selection is a confusing picture
                // to review a baseline against.
                let summary = "Installed (1) Community Patch, (2) Vox Populi, \
                               (3a) VP - EUI Compatibility Files, (4a) Squads for VP, VPUI, \
                               UI_bc1.";
                app.status = Status::Installed {
                    summary: summary.to_owned(),
                };
                app.activity = vec![
                    "Fetching sources: Mod files ready.".to_owned(),
                    "Building the DLL: DLL built.".to_owned(),
                    format!("Installing into the game: {summary}"),
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
        if !self.skinned {
            // Takes effect from the next frame, which every caller — the window, the
            // screenshot renderer, the kittest harness — runs plenty of before anyone looks.
            theme::apply(ui.ctx());
            self.skinned = true;
        }
        self.poll();
        deco::page(ui.style()).show(ui, |ui| {
            // Fill the window rather than shrink-wrapping the widgets, so the page
            // background covers the whole surface.
            ui.set_min_size(ui.available_size());
            self.contents(ui);
        });
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        deco::header(ui, "Civ 5 VP Installer");
        ui.label(
            egui::RichText::new(
                "Sources come from a local checkout, and the DLL build is a placeholder: the \
                 installed DLL is a marker file, not a compiled one.",
            )
            .small()
            .color(theme::PARCHMENT_DIM),
        );
        ui.add_space(6.0);

        let mut edited = false;
        deco::panel(ui, Some("Game folders"), |ui| {
            egui::Grid::new("folders")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    folder_field(ui, "Community-Patch-DLL folder", &mut self.source_folder);
                    // The two folders the installer detects and the player can correct. The
                    // three Deployment targets are not editable, because they are not separate
                    // choices: the Core derives them from these two.
                    edited |= folder_field(ui, "Civilization V game folder", &mut self.game_folder);
                    edited |= folder_field(
                        ui,
                        "Civilization 5 Documents folder",
                        &mut self.documents_folder,
                    );
                });

            ui.add_space(6.0);
            if let Ok(folders) = &self.resolved {
                deco::hairline(ui);
                for (label, path) in [
                    ("MODS folder", &folders.mods),
                    ("DLC folder", &folders.dlc),
                    ("Text folder", &folders.text),
                ] {
                    ui.label(
                        egui::RichText::new(format!("{label}: {}", path.display()))
                            .small()
                            .color(theme::PARCHMENT_DIM),
                    );
                }
            }
        });
        if edited {
            self.folders_changed();
        }

        if let Err(explanation) = &self.resolved {
            ui.add_space(6.0);
            deco::notice(ui, theme::EMBER, |ui| {
                ui.label(explanation);
            });
        }

        ui.add_space(6.0);
        let mut chosen = false;
        deco::panel(ui, Some("What to install"), |ui| {
            for (choice, label) in flavor_choices() {
                chosen |= ui.radio_value(&mut self.flavor, choice, label).changed();
            }
            ui.add_space(4.0);
            // 43 Civs is legal with either Flavor and with or without EUI, so it is the one
            // genuinely independent toggle.
            let mut enabled = self.forty_three_civs == FortyThreeCivs::Enabled;
            if ui
                .checkbox(&mut enabled, "43 Civs — room for 43 civilizations on a map")
                .changed()
            {
                self.forty_three_civs = if enabled {
                    FortyThreeCivs::Enabled
                } else {
                    FortyThreeCivs::Disabled
                };
                chosen = true;
            }
        });
        if chosen && self.resolved.is_ok() {
            // Remembered like the folders are, so the next launch starts from the same choice
            // (user story 26).
            self.remember();
        }

        ui.add_space(8.0);
        let busy = self.status == Status::Installing;
        let clicked = ui
            .vertical_centered(|ui| deco::primary_button(ui, !busy, "Install").clicked())
            .inner;
        if clicked {
            self.start_install();
        }

        ui.add_space(6.0);
        // The status, coloured by outcome — parchment at rest, gold while working, laurel on
        // success, ember on failure — with the deco progress bar while there is anything to
        // wait for. What the states mean is the Core's business; this is only their colour.
        let line = self.status_line();
        match &self.status {
            Status::Ready => {
                ui.label(egui::RichText::new(line).color(theme::PARCHMENT_DIM));
            }
            Status::Installing => {
                deco::progress(ui, None);
                ui.add_space(2.0);
                ui.label(egui::RichText::new(line).color(theme::GOLD_BRIGHT));
            }
            Status::Installed { .. } => {
                deco::progress(ui, Some(1.0));
                ui.add_space(2.0);
                ui.label(egui::RichText::new(line).color(theme::LAUREL));
            }
            Status::Failed { .. } => {
                deco::notice(ui, theme::EMBER, |ui| {
                    ui.label(line);
                });
            }
        }

        if !self.activity.is_empty() {
            ui.add_space(6.0);
            // The log takes whatever height is left and scrolls inside it, pinned to the
            // newest line, so the tail of the screen never spills past the window. The
            // subtraction is the panel's own chrome — padding, caption, spacing — and the
            // minimum keeps one line visible however small the window is forced.
            let room = (ui.available_height() - 48.0).max(17.0);
            deco::panel(ui, Some("Activity"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(room)
                    // The default minimum (64) would override `room` on a short window and
                    // push the panel's bottom edge past the page.
                    .min_scrolled_height(room)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for line in &self.activity {
                            ui.label(
                                egui::RichText::new(line)
                                    .small()
                                    .color(theme::PARCHMENT_DIM),
                            );
                        }
                    });
            });
        }

        if self.running.is_some() {
            // The worker thread has no way to wake the UI, so keep painting while it runs.
            // Keyed on a real running install, not on the status, so a preview of the
            // installing screen still settles into a still frame for the renderer.
            ui.ctx().request_repaint();
        }
    }

    /// Ask the Core what the two folders mean, now that one of them has been edited.
    fn folders_changed(&mut self) {
        self.resolved =
            resolve(&self.game_folder, &self.documents_folder).map_err(|r| r.user_message());
        if self.resolved.is_ok() {
            self.remember();
        }
    }

    /// Hand the current state to the App Data Store, so the next launch starts here.
    fn remember(&self) {
        let settings = Settings {
            game_installation: Some(PathBuf::from(self.game_folder.trim())),
            documents_folder: Some(PathBuf::from(self.documents_folder.trim())),
            configuration: Some(self.configuration()),
        };
        if let Err(problem) = self.store.save(&settings) {
            // Not being able to remember is not worth interrupting anything over: it goes in
            // the log, and the player finds out next launch (rule 11).
            crate::log_detail(&problem.log_detail());
        }
    }

    /// What the player has chosen, as the Core wants it.
    fn configuration(&self) -> InstallConfiguration {
        let source = if self.source_folder.trim().is_empty() {
            self.remembered_source.clone()
        } else {
            InstallationSource::LocalRepo {
                path: PathBuf::from(self.source_folder.trim()),
            }
        };
        InstallConfiguration {
            source,
            flavor: self.flavor.clone(),
            forty_three_civs: self.forty_three_civs,
        }
    }

    fn start_install(&mut self) {
        // The folders are judged before anything is fetched, built, or written, and the
        // judgement is the Core's (user story 12).
        let folders = match &self.resolved {
            Ok(folders) => folders.clone(),
            Err(explanation) => {
                self.status = Status::Failed {
                    message: explanation.clone(),
                };
                return;
            }
        };

        let configuration = self.configuration();
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
                // The configuration that worked is the one worth starting from next time.
                self.remember();
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

/// One row of the folder grid: a caption and the box it names. The two are tied together for
/// AccessKit, so a screen reader announces the box by its caption — which is also how the
/// shell tests find it, meaning they reach the field the same way a user does.
///
/// Returns whether the text changed this frame.
fn folder_field(ui: &mut egui::Ui, caption: &str, value: &mut String) -> bool {
    let caption = ui.label(caption);
    let field = ui
        .add(egui::TextEdit::singleline(value).desired_width(360.0))
        .labelled_by(caption.id);
    ui.end_row();
    field.changed()
}

/// Ask the Core what a pair of typed-in folders means, logging the detail either way (rule 11).
///
/// The rejection is handed back rather than turned into a sentence here, because the two
/// callers show different ones: a launch may have something more specific to say about this
/// machine than an edited field does.
fn resolve(game_folder: &str, documents_folder: &str) -> Result<GameFolders, FolderRejected> {
    resolve_game_folders(
        Path::new(game_folder.trim()),
        Path::new(documents_folder.trim()),
    )
    .inspect_err(|rejected| crate::log_detail(&rejected.log_detail()))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
