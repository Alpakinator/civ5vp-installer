//! The egui shell.
//!
//! Nothing here decides anything. It collects paths, hands them to the Core, and
//! renders what comes back. Which folders are Claimed, whether a Flavor is legal, what order
//! things happen in, whether a folder really is the game, where the MODS Folder lives inside
//! it, what to tell a player whose game cannot be used - all of that lives behind [`Core`] and
//! the Core's detection functions. Every sentence this file puts on screen either came out of
//! the Core or is a fixed label.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use civ5vp_core::{
    AppDataStore, BrowseField, BrowseRequest, BuildConfiguration, Core, DllSource, Eui, Flavor,
    FolderRejected, FortyThreeCivs, GameFolders, InstallConfiguration, InstallError, InstallMode,
    InstallationSource, LuaJitEngine, ProgressEvent, ProgressReporter, SearchLocations, Settings,
    Version, VersionCatalog, browse_start, home_directory, resolve_game_folders, start_up,
};

use crate::browse::{Browsing, FileSystemChoice};
use crate::{deco, placeholder, theme};

/// How many lines of the Activity log the panel is always tall enough to hold.
///
/// A fixed promise rather than a leftover. The log was previously sized from whatever height
/// the widgets above had not taken, and because the whole page sits in one `ScrollArea` that
/// is near zero on a full page - so it collapsed to a single line exactly when an install had
/// the most to report. Pinning it to the window bottom instead was tried and rejected: at the
/// design size it leaves too little for the configuration panels, which then clip mid-border.
/// The page scrolls, so a fixed height here is always honoured, if sometimes below the fold.
const MIN_ACTIVITY_LINES: f32 = 5.0;

/// Presentation state, not domain state - deliberately not public.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Ready,
    Installing,
    Installed { summary: String },
    Failed { message: String },
}

/// The screens `--screenshot` renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    FoldersNeeded,
    Ready,
    Installing,
    Installed,
    Failed,
    /// The Ready page with the file browser open over it - what a `Browse` click looks like.
    Browsing,
}

impl Screen {
    pub const ALL: [Self; 6] = [
        Self::FoldersNeeded,
        Self::Ready,
        Self::Installing,
        Self::Installed,
        Self::Failed,
        Self::Browsing,
    ];

    /// Used to name the PNG this screen renders to.
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::FoldersNeeded => "folders-needed",
            Self::Ready => "ready",
            Self::Installing => "installing",
            Self::Installed => "installed",
            Self::Failed => "failed",
            Self::Browsing => "browsing",
        }
    }
}

/// A Deployment or an Uninstall on the worker thread. The result is the finished sentence -
/// built by the worker from the Core's outcome - so both operations share one shape.
struct RunningInstall {
    progress: Receiver<ProgressEvent>,
    result: Receiver<Result<String, InstallError>>,
    /// When the click happened - the finished line reports the honest wall-clock cost,
    /// which on a first run is dominated by the Toolchain Bootstrap.
    started: std::time::Instant,
}

/// Which kind of Installation Source the player is using. Presentation state - the Core
/// receives a concrete [`InstallationSource`] either way and rules on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceChoice {
    /// The Upstream Cache: pick a Version, the installer downloads it.
    GitHub,
    /// Dev mode: a Local Repo, used as-is.
    OwnCheckout,
}

/// Where the Version list currently is.
enum VersionsState {
    NotAsked,
    /// The lookup thread's result lands here; `Err` is the sentence to show.
    Fetching(Receiver<Result<VersionCatalog, String>>),
    Ready(VersionCatalog),
    Failed(String),
}

/// What the player picked in the Version combo box.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PickedVersion {
    /// The default: whatever the newest Release turns out to be.
    NewestRelease,
    Release(String),
    /// One commit after the newest Release, from the unofficial list.
    Unofficial {
        label: String,
        commit: String,
    },
    /// Any branch, tag, or commit - the advanced escape hatch.
    Custom,
}

/// The unofficial-versions lookup - same life cycle as [`VersionsState`], but
/// started only when the toggle is on and the catalog has named the Releases to span.
enum UnofficialState {
    NotAsked,
    Fetching(Receiver<Result<Vec<civ5vp_core::UnofficialVersion>, String>>),
    Ready(Vec<civ5vp_core::UnofficialVersion>),
    Failed(String),
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
    source_choice: SourceChoice,
    versions: VersionsState,
    picked_version: PickedVersion,
    /// Whether the picker also lists the individual changes around the newest Releases.
    /// Off by default - official Releases are the offer; this is the opt-in.
    show_unofficial: bool,
    unofficial: UnofficialState,
    custom_ref: String,
    flavor: Flavor,
    forty_three_civs: FortyThreeCivs,
    /// Whether the player asked for the game's Lua engine to be replaced with LuaJIT.
    /// Off unless it was turned on: this is the only choice that overwrites a file the game
    /// owns, so a player has to reach for it rather than inherit it. Public so the choice can
    /// be set without a click - the tests prefer clicking, but nothing here needs hiding.
    pub luajit: bool,
    build_configuration: BuildConfiguration,
    /// Whether the player asked for the DLL to be compiled even where a Release ships a
    /// ready-made one. Off by default: the shipped DLL is the same file, minutes sooner, and
    /// without the Toolchain Bootstrap's download.
    compile_dll: bool,
    install_mode: InstallMode,
    /// The player's own MODS-folder mods a Modpack could bake in: what the
    /// Core found, and which of those are ticked. Recomputed when the folders resolve -
    /// listing means reading every modinfo, not a per-frame job.
    extra_mods_available: Vec<String>,
    extra_mods_picked: Vec<String>,
    activity: Vec<String>,
    status: Status,
    running: Option<RunningInstall>,
    /// Whether the art-deco theme has been installed on the context yet. Fonts and style
    /// are set once per context, not per frame - rebuilding the font atlas is not free.
    skinned: bool,
    /// The App Data Store's measured size, when the storage panel is open. `None` between
    /// looks - measuring a multi-gigabyte store is not a per-frame job.
    store_size: Option<u64>,
    /// The Core's up-front cost warning, drawn near Install while the first build would
    /// still bootstrap the toolchain. Refreshed when a Deployment finishes - that is the
    /// moment it can stop being true.
    first_run_note: Option<String>,
    /// The launch-time update ping's channel and answer. Wired only by the
    /// real binary - the shell tests and previews never open a socket.
    update_check: Option<Receiver<String>>,
    newer_installer: Option<String>,
    /// Where this machine keeps Steam, kept from the launch so a `Browse` click can ask the
    /// Core the same question detection asked.
    locations: SearchLocations,
    /// The file browser, while one is open. Presentation state: which window is up.
    browsing: Option<Browsing>,
}

/// The first `max` characters of a commit summary, with an ellipsis when it was cut -
/// dropdown rows are narrow and commit messages are not.
fn truncated(summary: &str, max: usize) -> String {
    let mut taken: String = summary.chars().take(max).collect();
    if summary.chars().count() > max {
        taken.push('…');
    }
    taken
}

/// How far a caption is pushed in to clear the radio button above it.
const RADIO_INDENT: f32 = 22.0;

/// How many Releases back the unofficial-versions list reaches.
///
/// Two: the changes since the newest Release, and the changes that *became* it. One was not
/// enough - right after a Release the list is empty, and the changes a player most wants to
/// look up are the ones that release just shipped. Each one costs a small round trip, which
/// is what keeps this from being "all of them".
const SPANNED_RELEASES: usize = 2;

/// The Flavor choices, as the player reads them.
///
/// There are three, not two-plus-a-checkbox, and that is the point: EUI is legal only with Vox
/// Populi, and listing the legal combinations means the shell cannot offer the illegal one
/// without a rule of its own to enforce. [`Flavor`] makes the same guarantee at the
/// type level; this is that guarantee drawn.
fn flavor_choices() -> [(Flavor, &'static str, Option<&'static str>); 3] {
    [
        (Flavor::CommunityPatch, "Community Patch only", None),
        (
            Flavor::VoxPopuli { eui: Eui::Disabled },
            "Community Patch + Vox Populi",
            None,
        ),
        (
            Flavor::VoxPopuli { eui: Eui::Enabled },
            "Community Patch + Vox Populi + EUI",
            // The other two names are the mods as people say them; this one is an
            // abbreviation, so it gets spelled out once underneath.
            Some("EUI - enhanced user interface"),
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

        // The remembered Installation Source, translated into picker state. A new player
        // starts on the GitHub path with the newest Release - the spec's default.
        let mut source_choice = SourceChoice::GitHub;
        let mut source_folder = String::new();
        let mut picked_version = PickedVersion::NewestRelease;
        let mut custom_ref = String::new();
        // A checkout named once is pre-filled forever, even while GitHub is the active
        // source - the field is only drawn in Dev mode, but it must not come back empty.
        if let Some(path) = &startup.dev_checkout {
            source_folder = display_path(path);
        }
        let mut show_unofficial = false;
        match startup.configuration.as_ref().map(|c| &c.source) {
            Some(InstallationSource::LocalRepo { path }) if !path.as_os_str().is_empty() => {
                source_choice = SourceChoice::OwnCheckout;
                source_folder = display_path(path);
            }
            Some(InstallationSource::UpstreamCache { version }) => match version {
                Version::Release(tag) => picked_version = PickedVersion::Release(tag.clone()),
                // The Latest Development Version is not a Version this picker offers, so
                // a configuration remembered before it stopped being one opens on the
                // default instead of on a choice that has no row to select. The newest
                // Unofficial Build is that same commit under a name that says what it is.
                Version::LatestDevelopmentVersion => {
                    picked_version = PickedVersion::NewestRelease;
                }
                Version::ArbitraryRef(reference) => {
                    picked_version = PickedVersion::Custom;
                    custom_ref = reference.clone();
                }
                Version::UnofficialBuild { label, commit } => {
                    picked_version = PickedVersion::Unofficial {
                        label: label.clone(),
                        commit: commit.clone(),
                    };
                    // A remembered unofficial pick means the player uses the toggle.
                    show_unofficial = true;
                }
            },
            _ => {}
        }
        let (
            flavor,
            forty_three_civs,
            luajit,
            build_configuration,
            compile_dll,
            install_mode,
            extra_mods_picked,
        ) = match &startup.configuration {
            Some(configuration) => (
                configuration.flavor.clone(),
                configuration.forty_three_civs,
                configuration.luajit == LuaJitEngine::LuaJit,
                configuration.build_configuration,
                configuration.dll_source == DllSource::AlwaysCompile,
                configuration.install_mode,
                configuration.extra_mods.clone(),
            ),
            None => (
                Flavor::suggested(),
                FortyThreeCivs::Disabled,
                false,
                BuildConfiguration::Release,
                false,
                InstallMode::Mods,
                Vec::new(),
            ),
        };
        let game_folder = startup
            .game_installation
            .as_deref()
            .map_or_else(String::new, display_path);
        let documents_folder = startup
            .documents_folder
            .as_deref()
            .map_or_else(String::new, display_path);

        // Start-up may have something more specific to say than a field-by-field rejection -
        // that the game is the native Aspyr port, say. Which of the two a player sees is the
        // Core's judgement, made by `Startup::explanation`, not the shell's.
        let resolved = resolve(&game_folder, &documents_folder)
            .map_err(|rejected| startup.explanation(&rejected));

        let first_run_note = core.first_run_expectation();
        let mut app = Self {
            core,
            store,
            source_choice,
            versions: VersionsState::NotAsked,
            picked_version,
            show_unofficial,
            unofficial: UnofficialState::NotAsked,
            custom_ref,
            source_folder,
            game_folder,
            documents_folder,
            resolved,
            flavor,
            forty_three_civs,
            luajit,
            build_configuration,
            compile_dll,
            install_mode,
            extra_mods_available: Vec::new(),
            extra_mods_picked,
            activity: Vec::new(),
            status: Status::Ready,
            running: None,
            skinned: false,
            store_size: None,
            first_run_note,
            update_check: None,
            newer_installer: None,
            locations: locations.clone(),
            browsing: None,
        };
        // Detected folders are worth remembering too: the next launch then starts from them
        // without searching.
        if app.resolved.is_ok() {
            app.remember();
        }
        app.refresh_extra_mods();
        app
    }

    /// An app frozen in one screen, for `--screenshot` and for snapshot baselines. Nothing is
    /// installed from a preview and nothing is remembered - it only ever gets rendered.
    pub fn preview(screen: Screen) -> Self {
        let store = AppDataStore::at(PathBuf::from("/preview/app-data"));
        let core = Arc::new(placeholder::core(store.root().to_path_buf()));
        let game = "/home/player/…/Sid Meier's Civilization V";
        let documents = "/home/player/…/Sid Meier's Civilization 5";
        let mut app = Self {
            core,
            store,
            source_choice: SourceChoice::GitHub,
            // A pre-loaded catalog: previews are pictures, they never open a socket.
            versions: VersionsState::Ready(placeholder::fixture_version_catalog()),
            picked_version: PickedVersion::NewestRelease,
            show_unofficial: false,
            unofficial: UnofficialState::NotAsked,
            custom_ref: String::new(),
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
                game_root: PathBuf::from(game),
            }),
            flavor: Flavor::suggested(),
            forty_three_civs: FortyThreeCivs::Disabled,
            // A picture of a first run, and a first run never replaces the game's engine.
            luajit: false,
            build_configuration: BuildConfiguration::Release,
            // A picture of the ordinary case: a Release install takes the DLL it ships.
            compile_dll: false,
            install_mode: InstallMode::Mods,
            extra_mods_available: Vec::new(),
            extra_mods_picked: Vec::new(),
            activity: Vec::new(),
            status: Status::Ready,
            running: None,
            skinned: false,
            store_size: None,
            first_run_note: None,
            update_check: None,
            newer_installer: None,
            // A preview never detects anything: the paths above are stated, not found.
            locations: SearchLocations::default(),
            browsing: None,
        };
        match screen {
            Screen::Ready => {}
            // The native Aspyr port: the refusal a player is most likely to meet and least
            // likely to work out unaided. The wording is the Core's, quoted.
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
                // Illustrative sample lines, not asserted against the Core's wording - a
                // preview never runs an install.
                app.activity = vec![
                    "Fetching sources: Getting the mod files ready.".to_owned(),
                    "Fetching sources: Mod files ready.".to_owned(),
                    "Building the DLL: Compiling 172 of 172 source files.".to_owned(),
                ];
            }
            Screen::Installed => {
                // The Flavor above is Vox Populi with EUI, so this says what that installs.
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
            // The Ready page with a browser over it. The folder tree it walks is a fixed
            // fake one (see `placeholder::preview_file_system`), so this picture is of the
            // browser rather than of whoever's machine rendered it.
            Screen::Browsing => {
                app.browsing = Some(Browsing::open(
                    BrowseField::Documents,
                    Some(PathBuf::from(placeholder::PREVIEW_BROWSER_DIRECTORY)),
                    FileSystemChoice::Fake(placeholder::preview_file_system()),
                ));
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
    /// also how the tests read it - there is no accessor for the shell's state, so the tests
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
    /// so all three show the same pixels - which is what makes a snapshot baseline mean
    /// anything about the shipped window.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if !self.skinned {
            // Takes effect from the next frame, which every caller - the window, the
            // screenshot renderer, the kittest harness - runs plenty of before anyone looks.
            theme::apply(ui.ctx());
            self.skinned = true;
        }
        self.poll();
        deco::page(ui.style()).show(ui, |ui| {
            // Fill the window rather than shrink-wrapping the widgets, so the page
            // background covers the whole surface.
            ui.set_min_size(ui.available_size());
            // The whole page scrolls: the extra-mod list and a long activity
            // log can outgrow any window, and on small screens even the base layout does.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                // The right page margin, held inside the scrolling area rather than outside
                // it: see [`deco::page`]. The bar is centred in it by [`theme`].
                .content_margin(egui::Margin {
                    right: deco::PAGE_MARGIN,
                    ..egui::Margin::ZERO
                })
                .show(ui, |ui| {
                    self.contents(ui);
                });
        });
        // Last, and outside the page: the browser is a window of its own, drawn over
        // everything the page just put on screen.
        self.update_browser(ui.ctx());
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        // The build's own version rides in the header's right corner, just above the
        // rule: the one moment anyone looks for it is while checking it against the
        // newer-version notice below, or while writing a bug report.
        deco::header(
            ui,
            "Civ 5 VP Installer",
            Some(&format!("v{}", crate::update::CURRENT_VERSION)),
        );
        ui.label(
            egui::RichText::new(
                "Pick a version to download, or point the installer at your own checkout. \
                 A release brings its own mod DLL; anything else is built here.",
            )
            .small()
            .color(theme::PARCHMENT_DIM),
        );
        if let Some(tag) = &self.newer_installer {
            // One sentence and a link, no auto-update machinery.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("A newer installer ({tag}) is available:"))
                        .color(theme::GOLD_BRIGHT),
                );
                ui.hyperlink_to("download it from GitHub", crate::update::RELEASES_PAGE_URL);
            });
        }
        // No added space here: the strap line under the rule belongs to the header, and the
        // panel's own margin is gap enough. The 6-point step between the panels below is
        // what separates sections from each other, not the header from the page.

        self.source_section(ui);
        ui.add_space(6.0);

        let mut edited = false;
        let mut browse = None;
        deco::panel(ui, Some("Game folders"), |ui| {
            // The two folders the installer detects and the player can correct. The three
            // Deployment targets are not editable, because they are not separate choices:
            // the Core derives them from these two.
            let game = folder_field(ui, GAME_FOLDER_CAPTION, &mut self.game_folder);
            let documents = folder_field(ui, DOCUMENTS_CAPTION, &mut self.documents_folder);
            edited |= game.edited | documents.edited;
            if game.browse {
                browse = Some(BrowseField::GameInstallation);
            }
            if documents.browse {
                browse = Some(BrowseField::Documents);
            }

            ui.add_space(6.0);
            if let Ok(folders) = &self.resolved {
                deco::hairline(ui);
                for (label, path) in [
                    // MODS and Text first - they are siblings in the Documents folder, so
                    // the two related paths sit together and the DLC one stands apart.
                    ("MODS folder", &folders.mods),
                    ("Text folder", &folders.text),
                    ("DLC folder", &folders.dlc),
                ] {
                    // Shortened on screen, whole everywhere it can be read in full. A real
                    // Proton path is long enough to wrap onto a second line, and the wrap
                    // lands mid-path - three facts become six lines of grey that read as
                    // damage. The middle is the part that carries nothing.
                    let full = format!("{label}: {}", path.display());
                    let line = ui
                        .label(
                            egui::RichText::new(format!("{label}: {}", elided_path(path)))
                                .small()
                                .color(theme::PARCHMENT_DIM),
                        )
                        .on_hover_text(&full);
                    // The whole path is what a screen reader announces and what the shell
                    // tests read, so nothing is actually hidden by the shortening.
                    ui.ctx().accesskit_node_builder(line.id, |node| {
                        node.set_label(full.clone());
                    });
                }
            }
        });
        if edited {
            self.folders_changed();
        }
        if let Some(field) = browse {
            self.open_browser(field);
        }

        if let Err(explanation) = &self.resolved {
            ui.add_space(6.0);
            let explanation = explanation.clone();
            deco::notice(ui, theme::EMBER, |ui| {
                ui.label(&explanation);
                self.support_buttons(ui, &explanation);
            });
        }

        ui.add_space(6.0);
        let mut chosen = false;
        deco::panel(ui, Some("What to install"), |ui| {
            for (choice, label, caption) in flavor_choices() {
                chosen |= deco::radio_value(ui, &mut self.flavor, choice, label).changed();
                if let Some(caption) = caption {
                    ui.horizontal(|ui| {
                        // Clear of the radio button, so the caption reads as belonging to
                        // the line above it rather than as a fourth choice.
                        ui.add_space(RADIO_INDENT);
                        ui.label(
                            egui::RichText::new(caption)
                                .small()
                                .color(theme::PARCHMENT_DIM),
                        );
                    });
                }
            }
            ui.add_space(4.0);
            // 43 Civs is legal with either Flavor and with or without EUI, so it is the one
            // genuinely independent toggle.
            let mut enabled = self.forty_three_civs == FortyThreeCivs::Enabled;
            if ui
                .checkbox(&mut enabled, "43 Civs - room for 43 civilizations on a map")
                .changed()
            {
                self.forty_three_civs = if enabled {
                    FortyThreeCivs::Enabled
                } else {
                    FortyThreeCivs::Disabled
                };
                chosen = true;
            }
            // The one choice that touches a file the game owns, so the hover text says what
            // gets replaced and what happens on uninstall. It also says what LuaJIT does
            // *not* do: ADR-0006 measured the claim, and Vox Populi's AI turn time is native
            // C++ that no Lua engine can speed up. Promising faster turns here would be a lie
            // the player only finds out about after a two-hour game.
            let engine = ui
                .checkbox(&mut self.luajit, "Use the LuaJIT engine")
                .on_hover_text(
                    "Replaces the game's Lua engine with LuaJIT. Map generation and the \
                     interface get faster; AI turn times are decided by the mod's C++ code \
                     and will not change. Your original file is saved, and put back if you \
                     clear this box or uninstall. Some older Lua mods do not work with it.",
                );
            if engine.changed() {
                chosen = true;
            }
            ui.add_space(4.0);
            // How the selection reaches the game. Two radios, not a checkbox:
            // "as mods" and "as a modpack" are both real things a player asks for by name.
            chosen |= deco::radio_value(
                ui,
                &mut self.install_mode,
                InstallMode::Mods,
                "Install as mods - activate them in the game's Mods menu",
            )
            .changed();
            chosen |= deco::radio_value(
                ui,
                &mut self.install_mode,
                InstallMode::Modpack,
                "Install as a modpack - loads automatically, works in multiplayer",
            )
            .changed();
            if self.install_mode == InstallMode::Modpack {
                ui.label(
                    egui::RichText::new(
                        "The modpack is baked into the game's DLC from a fresh copy of the \
                         game's data. Anything already in your MODS folder stays untouched.",
                    )
                    .small()
                    .color(theme::PARCHMENT_DIM),
                );
                if !self.extra_mods_available.is_empty() {
                    ui.add_space(4.0);
                    ui.label("Also bake in your own mods from the MODS folder:");
                    for name in self.extra_mods_available.clone() {
                        let mut picked = self.extra_mods_picked.contains(&name);
                        if ui.checkbox(&mut picked, &name).changed() {
                            if picked {
                                self.extra_mods_picked.push(name.clone());
                                self.extra_mods_picked.sort_unstable();
                            } else {
                                self.extra_mods_picked.retain(|kept| kept != &name);
                            }
                            chosen = true;
                        }
                    }
                }
            }
            ui.add_space(4.0);
            // Dev mode: pointing the installer at your own checkout is what makes you a mod
            // developer, and the Debug choice appears only then. The Core is
            // the one that *refuses* Debug anywhere else - this is just not drawing a
            // checkbox that could only be refused.
            if self.dev_mode() {
                let mut debug = self.build_configuration == BuildConfiguration::Debug;
                if ui
                    .checkbox(&mut debug, "Debug build - for stepping through the DLL")
                    .changed()
                {
                    self.build_configuration = if debug {
                        BuildConfiguration::Debug
                    } else {
                        BuildConfiguration::Release
                    };
                    chosen = true;
                }
            }
        });
        if chosen && self.resolved.is_ok() {
            // Remembered like the folders are, so the next launch starts from the same choice.
            self.remember();
        }

        ui.add_space(8.0);
        // The Core's up-front cost warning: the 1.1 GB first-run bootstrap
        // must be known before the click, and the sentence disappears the moment the
        // Toolchain Cache makes it untrue.
        //
        // Two conditions, both the Core's: the tools are not there yet, *and* this
        // configuration would have to compile something. A Release install taking the
        // Shipped DLL downloads nothing however empty the cache is, so warning about it
        // would send players away from the fast path for a cost they were never going to pay.
        // `needs_the_toolchain` errs towards showing it - a typed ref is not known to be a
        // Release until it is resolved - so the note is never missing when it is due.
        if self.configuration().needs_the_toolchain()
            && let Some(note) = &self.first_run_note
        {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(note.as_str())
                        .small()
                        .color(theme::PARCHMENT_DIM),
                );
            });
            ui.add_space(4.0);
        }
        let busy = self.status == Status::Installing;
        let (install_clicked, uninstall_clicked) = ui
            .vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    // Centre the pair by hand: total width = primary + gap + uninstall.
                    let free = ui.available_width();
                    ui.add_space((free - 220.0).max(0.0) / 2.0);
                    let install = deco::primary_button(ui, self.can_install(), "Install").clicked();
                    let uninstall =
                        deco::button(ui, !busy, egui::Button::new("Uninstall")).clicked();
                    (install, uninstall)
                })
                .inner
            })
            .inner;
        if install_clicked {
            self.start_install();
        }
        if uninstall_clicked {
            self.start_uninstall();
        }

        ui.add_space(6.0);
        // What the states mean is the Core's business; this is only their colour.
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
                    ui.label(line.clone());
                    // The full detail is in the log file; these put it in
                    // reach without raw compiler output ever entering the panel.
                    self.support_buttons(ui, &line);
                });
            }
        }

        ui.add_space(6.0);
        self.storage_section(ui);

        if !self.activity.is_empty() {
            self.activity_panel(ui);
        }

        if self.running.is_some() {
            // The worker thread has no way to wake the UI, so keep painting while it runs.
            // Keyed on a real running install, not on the status, so a preview of the
            // installing screen still settles into a still frame for the renderer.
            ui.ctx().request_repaint();
        }
    }

    /// The Activity log, at the foot of the page.
    ///
    /// Its height is [`MIN_ACTIVITY_LINES`] lines of the style it actually renders in - not a
    /// pixel constant - so it keeps its promise at any font scale. Scrolls within itself,
    /// stuck to the newest line.
    fn activity_panel(&self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let line = ui.text_style_height(&egui::TextStyle::Small) + ui.spacing().item_spacing.y;
        let room = line * MIN_ACTIVITY_LINES;
        deco::panel(ui, Some("Activity"), |ui| {
            egui::ScrollArea::vertical()
                .max_height(room)
                // The default minimum (64) would override `room` and make the panel taller
                // than the height it promises.
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
        ui.add_space(6.0);
    }

    /// A `Browse` click: ask the Core where to open, apply any correction it hands back,
    /// and put a folder picker on screen.
    ///
    /// Nothing about *where* is decided here. The ladder - the box, then detection, then the
    /// Documents side derived from the game folder, then home - is [`browse_start`]'s, and it
    /// is unit tested rung by rung behind the Core seam. The shell's whole part is reporting
    /// what the boxes hold and where this machine keeps Steam.
    fn open_browser(&mut self, field: BrowseField) {
        let home = home_directory();
        let start = browse_start(BrowseRequest {
            field,
            game_folder: Path::new(self.game_folder.trim()),
            documents_folder: Path::new(self.documents_folder.trim()),
            dev_checkout: Path::new(self.source_folder.trim()),
            locations: &self.locations,
            home: home.as_deref(),
        });
        // Written now rather than on picking, so cancelling the browser keeps the correction.
        // It can never overwrite a deliberate choice: the Core only reaches the rung that
        // produces one on a path that is not there.
        if let Some(correction) = start.correction {
            self.set_field(field, display_path(&correction));
        }
        self.browsing = Some(Browsing::open(
            field,
            start.directory,
            FileSystemChoice::Native,
        ));
    }

    /// Draw the file browser, if one is open, and take what it picked.
    ///
    /// A picked folder goes through exactly the same path as a typed one - it lands in the
    /// box and the Core is asked what it means - so a wrong pick is refused by the same
    /// inline notice, in the same words, as a wrong path typed in by hand.
    fn update_browser(&mut self, ctx: &egui::Context) {
        let Some(browsing) = &mut self.browsing else {
            return;
        };
        let field = browsing.field();
        let picked = browsing.update(ctx);
        let still_open = browsing.is_open();
        if let Some(picked) = picked {
            self.set_field(field, display_path(&picked));
            match field {
                BrowseField::GameInstallation | BrowseField::Documents => self.folders_changed(),
                BrowseField::DevCheckout if self.resolved.is_ok() => self.remember(),
                BrowseField::DevCheckout => {}
            }
        }
        if !still_open {
            self.browsing = None;
        }
    }

    /// The box a [`BrowseField`] names. The one place the shell maps the Core's idea of a
    /// field onto its own state, so a correction and a pick cannot disagree about which box
    /// they meant.
    fn set_field(&mut self, field: BrowseField, value: String) {
        match field {
            BrowseField::GameInstallation => self.game_folder = value,
            BrowseField::Documents => self.documents_folder = value,
            BrowseField::DevCheckout => self.source_folder = value,
        }
    }

    fn folders_changed(&mut self) {
        self.resolved =
            resolve(&self.game_folder, &self.documents_folder).map_err(|r| r.user_message());
        self.refresh_extra_mods();
        if self.resolved.is_ok() {
            self.remember();
        }
    }

    /// Re-list the player's own MODS-folder mods, and drop picks that no longer exist.
    fn refresh_extra_mods(&mut self) {
        self.extra_mods_available = match &self.resolved {
            Ok(folders) => civ5vp_core::available_extra_mods(&folders.mods),
            Err(_) => Vec::new(),
        };
        self.extra_mods_picked
            .retain(|name| self.extra_mods_available.contains(name));
    }

    fn remember(&self) {
        let mut configuration = self.configuration();
        // "Latest release" before the lookup has said which one that is. There is no Version
        // to write yet, and writing the placeholder would put a nameless release in the
        // settings file for the next launch to restore. Recording the source as unchosen is
        // what that file already means by "the player has not named one" - the Flavor and the
        // toggles beside it are still remembered, and the next launch opens on the newest
        // release, which is where this pick was pointing anyway.
        if matches!(
            &configuration.source,
            InstallationSource::UpstreamCache {
                version: Version::Release(tag),
            } if tag.is_empty()
        ) {
            configuration.source = InstallationSource::unchosen();
        }
        let settings = Settings {
            game_installation: Some(PathBuf::from(self.game_folder.trim())),
            documents_folder: Some(PathBuf::from(self.documents_folder.trim())),
            configuration: Some(configuration),
            // Kept separately from the configuration, which only stores the *active*
            // source: a player who names a checkout once must find it pre-filled even
            // after installing from GitHub in between.
            dev_checkout: match self.source_folder.trim() {
                "" => None,
                path => Some(PathBuf::from(path)),
            },
        };
        if let Err(problem) = self.store.save(&settings) {
            // Not being able to remember is not worth interrupting anything over: it goes in
            // the log, and the player finds out next launch.
            crate::log_detail(&problem.log_detail());
        }
    }

    /// Attach the launch-time update ping's answer channel. Only the real binary calls
    /// this; everything else never pings anything.
    pub fn with_update_check(mut self, receiver: Receiver<String>) -> Self {
        self.update_check = Some(receiver);
        self
    }

    /// The Installation Source: the Version picker for the GitHub path, or the checkout
    /// field for Dev mode. Which combinations are legal, and what a Version means, stay the
    /// Core's business - this draws choices and reports the pick.
    fn source_section(&mut self, ui: &mut egui::Ui) {
        let mut chosen = false;
        let mut browse = false;
        deco::panel(ui, Some("Install from"), |ui| {
            chosen |= deco::radio_value(
                ui,
                &mut self.source_choice,
                SourceChoice::GitHub,
                "Download from GitHub - pick a version",
            )
            .changed();
            chosen |= deco::radio_value(
                ui,
                &mut self.source_choice,
                SourceChoice::OwnCheckout,
                "My own Community-Patch-DLL checkout - Dev mode",
            )
            .changed();
            ui.add_space(4.0);
            match self.source_choice {
                SourceChoice::GitHub => chosen |= self.version_picker(ui),
                SourceChoice::OwnCheckout => {
                    let row = folder_field(ui, CHECKOUT_CAPTION, &mut self.source_folder);
                    chosen |= row.edited;
                    browse = row.browse;
                }
            }
        });
        if chosen && self.resolved.is_ok() {
            self.remember();
        }
        if browse {
            self.open_browser(BrowseField::DevCheckout);
        }
    }

    /// The Version combo and the states around it. Returns whether the pick changed.
    fn version_picker(&mut self, ui: &mut egui::Ui) -> bool {
        // The list is looked up once, lazily, on a thread - one round trip of ref names,
        // nothing downloaded. Offline it fails into a sentence and a retry button.
        if matches!(self.versions, VersionsState::NotAsked) {
            let (sender, receiver) = channel();
            let core = Arc::clone(&self.core);
            std::thread::spawn(move || {
                let looked_up = core
                    .available_versions(&ProgressReporter::silent())
                    .map_err(|error| {
                        crate::log_detail(&error.log_detail());
                        error.user_message()
                    });
                let _ = sender.send(looked_up);
            });
            self.versions = VersionsState::Fetching(receiver);
        }
        if let VersionsState::Fetching(receiver) = &self.versions {
            match receiver.try_recv() {
                Ok(Ok(catalog)) => self.versions = VersionsState::Ready(catalog),
                Ok(Err(message)) => self.versions = VersionsState::Failed(message),
                Err(TryRecvError::Empty) => {
                    // Keep painting until the lookup lands.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(TryRecvError::Disconnected) => {
                    self.versions = VersionsState::Failed(
                        "Could not look up the available versions. Check your internet \
                         connection and try again."
                            .to_owned(),
                    );
                }
            }
        }

        // The unofficial lookup: started once the toggle is on and the catalog has named the
        // Releases to span, polled like the catalog's own lookup. Two of them, so the list
        // covers the changes that became the newest Release as well as the ones since -
        // which release a given change first shipped in is exactly what someone opening this
        // toggle is trying to find out.
        let spanned: Vec<String> = match &self.versions {
            VersionsState::Ready(catalog) => catalog
                .releases()
                .iter()
                .take(SPANNED_RELEASES)
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        if self.show_unofficial
            && matches!(self.unofficial, UnofficialState::NotAsked)
            && !spanned.is_empty()
        {
            let (sender, receiver) = channel();
            let core = Arc::clone(&self.core);
            std::thread::spawn(move || {
                let found = core
                    .unofficial_versions(&spanned, &ProgressReporter::silent())
                    .map_err(|error| {
                        crate::log_detail(&error.log_detail());
                        error.user_message()
                    });
                let _ = sender.send(found);
            });
            self.unofficial = UnofficialState::Fetching(receiver);
        }
        if let UnofficialState::Fetching(receiver) = &self.unofficial {
            match receiver.try_recv() {
                Ok(Ok(list)) => self.unofficial = UnofficialState::Ready(list),
                Ok(Err(message)) => self.unofficial = UnofficialState::Failed(message),
                Err(TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(TryRecvError::Disconnected) => {
                    self.unofficial = UnofficialState::Failed(
                        "Could not look up the changes around the newest releases. Check \
                         your internet connection and try again."
                            .to_owned(),
                    );
                }
            }
        }

        // One name for one Version. A pick that names the newest Release *is* the "Latest …"
        // entry - it is what the settings file records for it, because that is the tag it
        // resolved to - and leaving it as a bare tag put the same install on screen under two
        // spellings: "Release-5.4.4" in the closed combo, "Latest Release-5.4.4" in the list
        // that is supposed to say which row is selected. Folded here rather than at launch
        // because which Release is newest is not known until the catalog lands.
        let picks_the_newest = match (&self.versions, &self.picked_version) {
            (VersionsState::Ready(catalog), PickedVersion::Release(tag)) => {
                catalog.newest_release() == Some(Version::Release(tag.clone()))
            }
            _ => false,
        };
        if picks_the_newest {
            self.picked_version = PickedVersion::NewestRelease;
        }

        let mut changed = false;
        match &self.versions {
            VersionsState::NotAsked | VersionsState::Fetching(_) => {
                ui.label(
                    egui::RichText::new("Looking up the available versions…")
                        .small()
                        .color(theme::PARCHMENT_DIM),
                );
            }
            VersionsState::Failed(message) => {
                let message = message.clone();
                deco::notice(ui, theme::EMBER, |ui| {
                    ui.label(&message);
                    self.support_buttons(ui, &message);
                });
                if deco::button(ui, true, egui::Button::new("Try again")).clicked() {
                    self.versions = VersionsState::NotAsked;
                }
            }
            VersionsState::Ready(catalog) => {
                let newest = catalog.newest_release();
                let selected_label = match &self.picked_version {
                    PickedVersion::NewestRelease => match &newest {
                        Some(Version::Release(tag)) => format!("Latest {tag}"),
                        _ => "Latest release".to_owned(),
                    },
                    PickedVersion::Release(tag) => tag.clone(),
                    PickedVersion::Unofficial { label, .. } => label.clone(),
                    PickedVersion::Custom => "Custom branch, tag, or commit".to_owned(),
                };
                // Cloned out so the combo's closure can mutate the pick freely.
                let unofficial: Vec<civ5vp_core::UnofficialVersion> =
                    match (&self.show_unofficial, &self.unofficial) {
                        (true, UnofficialState::Ready(list)) => list.clone(),
                        _ => Vec::new(),
                    };
                let releases: Vec<String> = catalog.releases().to_vec();
                // Built from an id rather than `from_label`, so that the response is the
                // box alone: `from_label`'s covers the caption beside it too, and a plate
                // painted to that rectangle would run out across the panel. The caption is
                // drawn and tied on below, exactly as `from_label` would have.
                //
                // The blanking is undone at the top of the dropdown's own closure: the rows
                // in the list are ordinary widgets and need their real colours back to show
                // which one the pointer is on.
                let dropdown = ui.visuals().clone();
                ui.horizontal(|ui| {
                    let plate = deco::plate(ui);
                    let combo = ui
                        .scope(|ui| {
                            deco::blank_frames(ui);
                            egui::ComboBox::from_id_salt("version")
                                .selected_text(selected_label)
                                .show_ui(ui, |ui| {
                                    *ui.visuals_mut() = dropdown;
                                    if let Some(Version::Release(tag)) = &newest {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.picked_version,
                                                PickedVersion::NewestRelease,
                                                format!("Latest {tag}"),
                                            )
                                            .changed();
                                    }
                                    // Unofficial versions, newest first - the top entry is
                                    // what "latest development version" used to mean. The whole commit
                                    // message never fits a dropdown row, so the row truncates and the
                                    // full message is the hover text.
                                    for build in unofficial.iter().rev() {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.picked_version,
                                                PickedVersion::Unofficial {
                                                    label: build.label.clone(),
                                                    commit: build.commit.clone(),
                                                },
                                                format!(
                                                    "{} - {}",
                                                    build.label,
                                                    truncated(&build.summary, 44)
                                                ),
                                            )
                                            .on_hover_text(format!(
                                                "{}\n{}",
                                                build.summary,
                                                &build.commit[..build.commit.len().min(12)]
                                            ))
                                            .changed();
                                    }
                                    // The newest release is already the "Latest Release-…" entry.
                                    let newest_tag = match &newest {
                                        Some(Version::Release(tag)) => Some(tag.as_str()),
                                        _ => None,
                                    };
                                    for tag in releases
                                        .iter()
                                        .filter(|tag| Some(tag.as_str()) != newest_tag)
                                    {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.picked_version,
                                                PickedVersion::Release(tag.clone()),
                                                tag,
                                            )
                                            .changed();
                                    }
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.picked_version,
                                            PickedVersion::Custom,
                                            "Custom branch, tag, or commit",
                                        )
                                        .changed();
                                })
                        })
                        .inner;
                    plate.settle(ui, &combo.response);
                    let caption = ui.label("Version");
                    combo.response.labelled_by(caption.id);
                });
                if self.picked_version == PickedVersion::Custom {
                    ui.horizontal(|ui| {
                        ui.label("Ref:");
                        changed |=
                            deco::text_field(ui, egui::TextEdit::singleline(&mut self.custom_ref))
                                .changed();
                    });
                }
                // Only where it can change anything. A Release ships the DLL it was built
                // with, and a typed ref might name one; every other pick compiles regardless,
                // and a box that does nothing is worse than no box.
                if matches!(
                    self.picked_version,
                    PickedVersion::NewestRelease
                        | PickedVersion::Release(_)
                        | PickedVersion::Custom
                ) {
                    changed |= ui
                        .checkbox(&mut self.compile_dll, "Compile the DLL myself")
                        .on_hover_text(
                            "A release ships the DLL it was built from, so the installer \
                             deploys that one and has nothing to compile. Tick this to build \
                             it here instead - the first build downloads the build tools, \
                             which takes a while.",
                        )
                        .changed();
                }
                ui.checkbox(
                    &mut self.show_unofficial,
                    "Unofficial versions - every change since the release before last",
                );
                if self.show_unofficial
                    && let UnofficialState::Ready(list) = &self.unofficial
                    && list.is_empty()
                {
                    // Master sitting exactly at the newest Release is normal right after
                    // one ships - say so, or an empty-looking toggle reads as broken.
                    ui.label(
                        egui::RichText::new("There are no unofficial versions to list yet.")
                            .small()
                            .color(theme::PARCHMENT_DIM),
                    );
                }
                if self.show_unofficial
                    && let UnofficialState::Failed(message) = &self.unofficial
                {
                    let message = message.clone();
                    deco::notice(ui, theme::EMBER, |ui| {
                        ui.label(&message);
                    });
                    if deco::button(ui, true, egui::Button::new("Try again")).clicked() {
                        self.unofficial = UnofficialState::NotAsked;
                    }
                }
            }
        }
        changed
    }

    /// The concrete Version the picker means right now.
    ///
    /// `NewestRelease` needs the catalog; [`Self::can_install`] keeps the Install button
    /// disabled until it is there, so the fallback below is never what actually installs.
    fn effective_version(&self) -> Version {
        match &self.picked_version {
            PickedVersion::Release(tag) => Version::Release(tag.clone()),
            PickedVersion::Unofficial { label, commit } => Version::UnofficialBuild {
                label: label.clone(),
                commit: commit.clone(),
            },
            PickedVersion::Custom => Version::ArbitraryRef(self.custom_ref.trim().to_owned()),
            // A Release with no name, until the catalog says which one. `can_install`
            // keeps it off the install path; if it ever got there the source provider would
            // refuse it by name, which is the right failure. It must not fall back to
            // `master` - "latest release" quietly installing the development version is the
            // one wrong answer available here.
            PickedVersion::NewestRelease => match &self.versions {
                VersionsState::Ready(catalog) => catalog
                    .newest_release()
                    .unwrap_or_else(|| Version::Release(String::new())),
                _ => Version::Release(String::new()),
            },
        }
    }

    /// Whether the Install button means what the screen says it means.
    fn can_install(&self) -> bool {
        if self.status == Status::Installing {
            return false;
        }
        // "Latest release" must not quietly install something else while the lookup is
        // still out (or failed) - wait until the catalog says which release that is.
        !(self.source_choice == SourceChoice::GitHub
            && self.picked_version == PickedVersion::NewestRelease
            && !matches!(&self.versions, VersionsState::Ready(_)))
    }

    /// The storage panel: where the App Data Store is, how large it is, and
    /// the one button that clears it. Everything it shows and does comes from
    /// [`AppDataStore`]; the game is never involved.
    fn storage_section(&mut self, ui: &mut egui::Ui) {
        let location = display_path(self.store.root());
        let response = egui::CollapsingHeader::new("Storage")
            .default_open(false)
            .show(ui, |ui| {
                if self.store_size.is_none() {
                    self.store_size = Some(self.store.size_on_disk());
                }
                ui.label(
                    egui::RichText::new(format!(
                        "The installer keeps its downloads, build files and settings in \
                         {location} - currently {}.",
                        human_size(self.store_size.unwrap_or(0)),
                    ))
                    .small(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let busy = self.status == Status::Installing;
                    if deco::button(ui, !busy, egui::Button::new("Clear stored data")).clicked() {
                        match self.store.clear() {
                            Ok(()) => {
                                self.store_size = None;
                                self.activity.push(
                                    "Storage: Cleared the installer's stored data. The next \
                                     install will download and set up everything again."
                                        .to_owned(),
                                );
                            }
                            Err(problem) => {
                                crate::log_detail(&problem.log_detail());
                                self.status = Status::Failed {
                                    message: problem.user_message(),
                                };
                            }
                        }
                    }
                    if deco::button(ui, true, egui::Button::new("Recalculate size")).clicked() {
                        self.store_size = None;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Clearing never touches the game or its mods - only the installer's \
                         own folder.",
                    )
                    .small()
                    .color(theme::PARCHMENT_DIM),
                );
            });
        // While the panel is closed the size is left unknown, so a 5 GB walk does not run
        // just to draw a collapsed header - and a stale number never lingers either.
        if response.body_response.is_none() {
            self.store_size = None;
        }
    }

    /// The copy/open row every failure surface carries. `headline` is what the notice says;
    /// "Copy details" puts it on the clipboard plus the tail of the log file - enough for a
    /// useful report, small enough to paste anywhere.
    fn support_buttons(&self, ui: &mut egui::Ui, headline: &str) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if deco::button(ui, true, egui::Button::new("Copy details")).clicked() {
                ui.ctx().copy_text(self.log_text_for_report(headline));
            }
            if deco::button(ui, true, egui::Button::new("Open log")).clicked() {
                self.open_log();
            }
            if let Some(path) = crate::log_file() {
                ui.label(
                    egui::RichText::new(display_path(path))
                        .small()
                        .color(theme::PARCHMENT_DIM),
                );
            }
        });
    }

    fn log_text_for_report(&self, headline: &str) -> String {
        let mut report = headline.to_owned();
        if let Some(path) = crate::log_file()
            && let Ok(contents) = std::fs::read_to_string(path)
        {
            let lines: Vec<&str> = contents.lines().collect();
            let tail = lines.len().saturating_sub(120);
            report.push_str("\n\n--- log tail ---\n");
            for line in &lines[tail..] {
                report.push_str(line);
                report.push('\n');
            }
        }
        report
    }

    fn open_log(&self) {
        if let Some(path) = crate::log_file() {
            crate::open_path(path);
        }
    }

    /// Dev mode is "building from your own checkout": a Local Repo has been named, in the
    /// field or remembered from last time. What Dev mode *permits* is the Core's ruling;
    /// this only decides which widgets are worth drawing.
    fn dev_mode(&self) -> bool {
        self.source_choice == SourceChoice::OwnCheckout
    }

    fn configuration(&self) -> InstallConfiguration {
        let source = match self.source_choice {
            SourceChoice::OwnCheckout => InstallationSource::LocalRepo {
                path: PathBuf::from(self.source_folder.trim()),
            },
            SourceChoice::GitHub => InstallationSource::UpstreamCache {
                version: self.effective_version(),
            },
        };
        InstallConfiguration {
            source,
            flavor: self.flavor.clone(),
            forty_three_civs: self.forty_three_civs,
            install_mode: self.install_mode,
            extra_mods: self.extra_mods_picked.clone(),
            // The checkbox, translated. Untouched it asks for the engine the game shipped
            // with - what a Replaced File costs is the Core's business, but whether one is
            // wanted at all is the player's, and silence means no.
            luajit: if self.luajit {
                LuaJitEngine::LuaJit
            } else {
                LuaJitEngine::Stock
            },
            // Sent as chosen, even when Dev mode is off and the checkbox is not drawn:
            // which Build Configurations are legal with which sources is the Core's ruling,
            // and it refuses an illegal pair with a sentence.
            build_configuration: self.build_configuration,
            // Likewise: whether a Version has a Shipped DLL worth taking is settled by the
            // Core once the sources are in hand, so the box only ever says "compile anyway".
            dll_source: if self.compile_dll {
                DllSource::AlwaysCompile
            } else {
                DllSource::ShippedWhenCurrent
            },
        }
    }

    fn start_install(&mut self) {
        // The folders are judged before anything is fetched, built, or written, and the
        // judgement is the Core's.
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
            let finished = core.execute(&plan, &reporter).map(|outcome| {
                let names: Vec<_> = outcome
                    .deployed
                    .iter()
                    .map(|folder| folder.folder_name())
                    .collect();
                format!("Installed {}.", names.join(", "))
            });
            let _ = result_sender.send(finished);
        });

        self.activity.clear();
        self.status = Status::Installing;
        self.running = Some(RunningInstall {
            progress,
            result,
            started: std::time::Instant::now(),
        });
    }

    /// Back to an unmodded game in one click. Same worker shape as an
    /// install; what gets removed is the Core's ruling.
    fn start_uninstall(&mut self) {
        let folders = match &self.resolved {
            Ok(folders) => folders.clone(),
            Err(explanation) => {
                self.status = Status::Failed {
                    message: explanation.clone(),
                };
                return;
            }
        };

        let (progress_sender, progress) = channel();
        let (result_sender, result) = channel();
        let core = Arc::clone(&self.core);
        std::thread::spawn(move || {
            let reporter = ProgressReporter::to_channel(progress_sender);
            let finished = core.uninstall(&folders, &reporter).map(|outcome| {
                if outcome.removed.is_empty() && outcome.removed_files.is_empty() {
                    "Nothing of Vox Populi was installed - your game is already unmodded."
                        .to_owned()
                } else {
                    let names: Vec<_> = outcome
                        .removed
                        .iter()
                        .map(|folder| folder.folder_name())
                        .collect();
                    format!(
                        "Removed {}. Your game is back to how it was.",
                        names.join(", ")
                    )
                }
            });
            let _ = result_sender.send(finished);
        });

        self.activity.clear();
        self.status = Status::Installing;
        self.running = Some(RunningInstall {
            progress,
            result,
            started: std::time::Instant::now(),
        });
    }

    /// Drain whatever the worker thread has produced since the last frame.
    fn poll(&mut self) {
        if let Some(check) = &self.update_check
            && let Ok(tag) = check.try_recv()
        {
            self.newer_installer = Some(tag);
            self.update_check = None;
        }
        let Some(run) = self.running.take() else {
            return;
        };

        let mut lines = Vec::new();
        while let Ok(event) = run.progress.try_recv() {
            lines.push(format!("{}: {}", event.stage.label(), event.message));
        }

        let finished = match run.result.try_recv() {
            Ok(Ok(summary)) => {
                self.status = Status::Installed { summary };
                // A finished Deployment is the moment the first-run warning can stop
                // being true - ask again rather than guess.
                self.first_run_note = self.core.first_run_expectation();
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
            // The worker died without sending a result, so there is no Core error to quote -
            // this is the one message the shell has to author itself.
            Err(TryRecvError::Disconnected) => {
                self.status = Status::Failed {
                    message: "This stopped unexpectedly - check the log for what happened."
                        .to_owned(),
                };
                true
            }
        };

        if finished {
            // The worker reports its result last, but events it sent just before that may
            // still be sitting in the channel. Without this the tail of Sync - the lines
            // saying what was actually installed - would be dropped.
            while let Ok(event) = run.progress.try_recv() {
                lines.push(format!("{}: {}", event.stage.label(), event.message));
            }
            // A failed run that signs off with "Finished" is how a user comes to believe the
            // mod is installed when it is not - the red notice above says otherwise, but the
            // last line of the log is what gets read and reported.
            let elapsed = elapsed_label(run.started.elapsed());
            lines.push(match &self.status {
                Status::Failed { .. } => format!("Stopped after {elapsed} without finishing."),
                _ => format!("Finished in {elapsed}."),
            });
        }

        self.activity.extend(lines);
        if !finished {
            self.running = Some(run);
        }
    }
}

/// What one folder row reported this frame.
#[derive(Default)]
struct FolderRow {
    /// The text in the box changed.
    edited: bool,
    /// `Browse` was clicked.
    browse: bool,
}

/// The three captions the folder rows carry, and the source of truth for how wide their
/// column is. A row whose caption is not in this list would be measured against a column
/// sized without it, and would line up with nothing - so add here as well as at the call
/// site.
const GAME_FOLDER_CAPTION: &str = "Civilization V game folder";
const DOCUMENTS_CAPTION: &str = "Civilization 5 Documents folder";
const CHECKOUT_CAPTION: &str = "Community-Patch-DLL folder";
const FOLDER_CAPTIONS: [&str; 3] = [GAME_FOLDER_CAPTION, DOCUMENTS_CAPTION, CHECKOUT_CAPTION];

/// How wide the caption column has to be: the widest of the three, measured in the font the
/// window is actually using.
///
/// Measured rather than written down. A constant that clears the longest caption by six
/// points today stops clearing it the moment the font, its size, or the user's scaling
/// changes - and what it fails into is a caption sitting on top of the path box.
fn caption_column(ui: &egui::Ui) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    FOLDER_CAPTIONS
        .iter()
        .map(|caption| {
            ui.painter()
                .layout_no_wrap((*caption).to_owned(), font.clone(), theme::PARCHMENT)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        .ceil()
}

/// The fixed width the button is given. The path box takes what is left, and a box sized from
/// the space around it, inside a container sized from the box, is a feedback loop that
/// settles at a different width on every row.
const BROWSE_BUTTON_WIDTH: f32 = 48.0;

/// The button's own padding, tighter than the page's: it is a small control beside a box, not
/// one of the page's own buttons, and at the page's padding it stands taller than the box it
/// belongs to.
const BROWSE_BUTTON_PADDING: egui::Vec2 = egui::vec2(3.0, 1.0);

/// How tall the button is: a little under the path box beside it, so the box stays the thing
/// the row is about.
const BROWSE_BUTTON_HEIGHT: f32 = 24.0;

/// The narrowest the path box is allowed to get before the row simply overflows. A window
/// squeezed below this has bigger problems than a clipped path.
const MIN_PATH_WIDTH: f32 = 160.0;

/// One row of the folder panel: a caption, the box it names, and the button that opens a
/// file browser for it.
///
/// The caption and the box are tied together for AccessKit, so a screen reader announces the
/// box by its caption - which is also how the shell tests find it, meaning they reach the
/// field the same way a user does. The button reads only `Browse` on screen, because three
/// of them stacked read as a column rather than as three separate offers; its *accessible*
/// name says which folder it is for, so a screen reader - and the tests - can tell the three
/// apart without the window having to.
fn folder_field(ui: &mut egui::Ui, caption: &str, value: &mut String) -> FolderRow {
    let mut row = FolderRow::default();
    let caption_width = caption_column(ui);
    // Measured out here, from the panel, rather than inside the row: a horizontal layout
    // reports the width it *could* grow to, and a row sized from that grows the panel it is
    // in, one row at a time, until nothing lines up with anything.
    let gap = ui.spacing().item_spacing.x;
    // The row fills the panel's content width exactly: caption, gap, box, gap, button. Going
    // over it is not a small mistake - the panel grows to fit the row, which widens the
    // content area, which widens the row again, and every panel on the page ends up drawn
    // out under the page's scroll bar.
    let width = (ui.available_width() - caption_width - BROWSE_BUTTON_WIDTH - 2.0 * gap)
        .max(MIN_PATH_WIDTH);
    let height = ui.spacing().interact_size.y;
    ui.horizontal(|ui| {
        let label = ui
            .allocate_ui_with_layout(
                egui::vec2(caption_width, height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // Without this the caption column shrink-wraps its text, and the three
                    // rows - whose captions are three different lengths - line up with
                    // nothing at all.
                    ui.set_min_width(caption_width);
                    ui.label(caption)
                },
            )
            .inner;
        let field = deco::text_field(ui, egui::TextEdit::singleline(value).desired_width(width))
            .labelled_by(label.id);
        ui.spacing_mut().button_padding = BROWSE_BUTTON_PADDING;
        let browse = deco::button(
            ui,
            true,
            egui::Button::new(egui::RichText::new(BROWSE_LABEL).small())
                .min_size(egui::vec2(BROWSE_BUTTON_WIDTH, BROWSE_BUTTON_HEIGHT)),
        );
        let spoken = format!("{BROWSE_LABEL} for the {caption}");
        ui.ctx().accesskit_node_builder(browse.id, |node| {
            node.set_label(spoken.clone());
        });
        row = FolderRow {
            edited: field.changed(),
            browse: browse.clicked(),
        };
    });
    row
}

/// What the button says on screen. The accessible name adds the folder - see [`folder_field`].
const BROWSE_LABEL: &str = "Browse";

/// Ask the Core what a pair of typed-in folders means, logging the detail either way.
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

/// A duration as a player reads one: "47 s", "12 min 30 s", "1 h 08 min".
fn elapsed_label(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    match (seconds / 3600, (seconds % 3600) / 60, seconds % 60) {
        (0, 0, s) => format!("{s} s"),
        (0, m, s) => format!("{m} min {s} s"),
        (h, m, _) => format!("{h} h {m:02} min"),
    }
}

/// Bytes as a player reads them. One decimal place, the unit that keeps the number small.
fn human_size(bytes: u64) -> String {
    const UNITS: [(&str, u64); 3] = [("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10)];
    for (unit, size) in UNITS {
        if bytes >= size {
            return format!("{:.1} {unit}", bytes as f64 / size as f64);
        }
    }
    format!("{bytes} bytes")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A path with its middle taken out: the first few components, an ellipsis, the last few.
///
/// What a player checks in a derived path is the two ends - that it is under the right home
/// or drive, and that it lands on the right folder. Everything between is nine levels of
/// Proton prefix that is the same on every machine that has one.
fn elided_path(path: &Path) -> String {
    /// Components kept at each end. Three at the front covers a root and two names -
    /// `/home/sunny`, or `C:\Users\sunny` - and three at the back covers
    /// `My Games/Sid Meier's Civilization 5/MODS`.
    const KEPT: usize = 3;

    let components: Vec<_> = path.components().collect();
    // Nothing to gain unless at least two components would come out.
    if components.len() <= KEPT * 2 + 1 {
        return display_path(path);
    }
    let mut short = PathBuf::new();
    short.extend(&components[..KEPT]);
    short.push("…");
    short.extend(&components[components.len() - KEPT..]);
    display_path(&short)
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `elided_path` rebuilds the path through `PathBuf`, which renders with the platform's
    /// own separator - `\` on Windows, `/` everywhere else. Both are correct, and nothing a
    /// player reads depends on which. Comparing against one written spelling therefore means
    /// normalising the separator rather than asserting which platform the test runs on.
    fn with_forward_slashes(path: String) -> String {
        path.replace('\\', "/")
    }

    /// The two ends survive and the middle goes: exactly the two things a player checks in a
    /// derived path - that it is under the right home, and that it lands on the right folder.
    #[test]
    fn a_long_path_keeps_its_two_ends() {
        let path = Path::new(
            "/home/sunny/chest/SteamLibrary/steamapps/compatdata/8930/pfx/drive_c/users\
             /steamuser/Documents/My Games/Sid Meier's Civilization 5/MODS",
        );
        assert_eq!(
            with_forward_slashes(elided_path(path)),
            "/home/sunny/…/My Games/Sid Meier's Civilization 5/MODS",
        );
    }

    /// A path short enough to read is left exactly as it is - shortening it would cost a
    /// component and save nothing.
    #[test]
    fn a_short_path_is_left_alone() {
        let path = Path::new("/home/sunny/Games/Civ/MODS");
        assert_eq!(
            with_forward_slashes(elided_path(path)),
            "/home/sunny/Games/Civ/MODS"
        );
    }

    /// The boundary. Seven components - the root counts as one - is the longest that is left
    /// whole, because taking a single component out would only replace it with an ellipsis.
    #[test]
    fn nothing_is_taken_out_unless_two_components_go() {
        assert_eq!(
            with_forward_slashes(elided_path(Path::new("/a/b/c/d/e/f"))),
            "/a/b/c/d/e/f"
        );
        assert_eq!(
            with_forward_slashes(elided_path(Path::new("/a/b/c/d/e/f/g"))),
            "/a/b/…/e/f/g"
        );
    }
}
