//! The in-app file browser behind the `Browse` buttons.
//!
//! A folder picker drawn inside the window, not a native dialog. That is a rule-5 decision
//! (CODING_STANDARDS.md: "assume the user's machine has nothing"): every portable Rust
//! wrapper over the platform's own dialog falls back to running `zenity` on Linux when no XDG
//! desktop portal is present, or needs GTK development headers to build and `libgtk-3.so` at
//! run time. Either would be the first external process the installer expects a user to have.
//! Two lesser reasons point the same way: a native dialog is invisible to `egui_kittest`, so
//! the button could not be verified by rendering (rule 15), and a blocking native call inside
//! an egui frame freezes the window while it is up.
//!
//! Where the browser *opens* is not decided here - that is the Core's
//! [`civ5vp_core::browse_start`] ladder. This module only builds the dialog and keeps track of
//! which field is being browsed for.

use std::path::PathBuf;
use std::sync::Arc;

use civ5vp_core::BrowseField;
use egui_file_dialog::{DialogState, FileDialog, FileDialogStorage, FileSystem, OpeningMode};

/// An open file browser, and the field whose `Browse` button opened it.
pub struct Browsing {
    field: BrowseField,
    dialog: FileDialog,
}

impl Browsing {
    /// Open a folder picker at `directory`, for `field`.
    ///
    /// Every setting below is load-bearing:
    ///
    /// * `OpeningMode::AlwaysInitialDir` - the default is `LastPickedDir`, which demotes
    ///   `initial_directory` to a fallback used only on the first open. With the default the
    ///   whole ladder silently stops firing after one pick, and it looks like it works,
    ///   because the first use does open in the right place.
    /// * `canonicalize_paths(false)` - the default is on, and on Windows it turns the pick
    ///   into a `\\?\C:\…` extended-length path. That string would go into the text box and
    ///   into `settings.txt`, both of which a player reads. The cost is knowingly accepted: a
    ///   symlinked Steam library stays symlinked.
    /// * `show_hidden: true` - hidden files are off by default, and on Linux Steam lives at
    ///   `~/.steam/steam` and `~/.local/share/Steam`. Nothing is remembered between uses, so
    ///   this has to be set every time rather than restored from storage.
    /// * `as_modal(true)` - the page behind the browser is full of live controls, and a
    ///   picker that lets you edit the box it is filling in is a picker that can be raced.
    pub fn open(
        field: BrowseField,
        directory: Option<PathBuf>,
        file_system: FileSystemChoice,
    ) -> Self {
        let mut dialog = match file_system {
            FileSystemChoice::Native => FileDialog::new(),
            FileSystemChoice::Fake(file_system) => FileDialog::with_file_system(file_system),
        }
        .title(title(field))
        // What the top strip is allowed to hold. Each of these is dead weight in a folder
        // picker that exists to find one folder that already exists:
        //
        // * the search box is 140 fixed points of the widest thing on the row, and what it
        //   filters is one folder's listing - while the breadcrumb next to it, which is a
        //   path nine levels deep, is the thing actually starved of width. Its field is also
        //   the black rectangle the crate paints with a hardcoded `dark_canvas`.
        // * `+` makes a new folder. Nobody creates the Civilization V install folder here.
        // * "select all" is meaningless when the answer is one directory, and "working
        //   directory" means the installer's own, which is wherever it was launched from.
        //
        // What stays: back, forward, parent, the breadcrumb, the pencil that lets a path be
        // pasted in, and a menu holding reload and the hidden-files toggle.
        .show_search(false)
        .show_menu_button(false)
        .show_new_folder_button(false)
        .show_select_all_button(false)
        .show_working_directory_button(false)
        // Shrink-wrapped, the window is a letterbox with a handful of rows in it. This is
        // room to actually walk a folder tree, and it stays resizable.
        .default_size(BROWSER_SIZE)
        .min_size(BROWSER_MIN_SIZE)
        // Centred rather than wherever egui would put a new window: it is modal, so there is
        // nothing beside it to make room for, and a picker that opens half off the top-left
        // corner reads as a mistake. The cost is that it cannot be dragged.
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .opening_mode(OpeningMode::AlwaysInitialDir)
        .canonicalize_paths(false)
        .as_modal(true)
        .storage(FileDialogStorage {
            show_hidden: true,
            ..FileDialogStorage::default()
        });
        if let Some(directory) = directory {
            dialog = dialog.initial_directory(directory);
        }
        dialog.pick_directory();
        Self { field, dialog }
    }

    pub fn field(&self) -> BrowseField {
        self.field
    }

    /// Draw the browser and report a folder if one was picked this frame.
    ///
    /// `None` covers both "still open" and "cancelled"; the caller distinguishes them with
    /// [`Self::is_open`], because a cancelled browser has to be dropped and an open one has
    /// to be kept.
    pub fn update(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        // The page's scroll bars float over their content, centred in the margin
        // `deco::page` leaves them. Inside the browser that is wrong twice over: the
        // breadcrumb bar is a *horizontal* scrolling area one button tall, so a floating bar
        // lies across the folder names it is meant to scroll, and there is no page margin in
        // here for an outer margin to centre a bar in. A solid bar takes a strip of its own
        // along the bottom edge instead.
        //
        // Swapped on the context rather than passed in, because the whole browser is drawn
        // inside the crate's `update` and nothing of ours reaches into it. It is put back
        // immediately, and this is the last thing the frame draws.
        let outer = ctx.style_of(egui::Theme::Dark);
        ctx.set_style_of(egui::Theme::Dark, browser_style(&outer));
        self.dialog.update(ctx);
        ctx.set_style_of(egui::Theme::Dark, outer);
        self.dialog.take_picked()
    }

    /// Whether the browser is still up. Once it is not, the caller drops it - a cancelled
    /// browser and a finished one are the same thing to the page behind it.
    pub fn is_open(&self) -> bool {
        *self.dialog.state() == DialogState::Open
    }
}

/// Which file system the browser walks. The previews and their snapshot baselines use a fixed
/// fake tree, so the sixth screen renders the same PNG on any machine.
pub enum FileSystemChoice {
    Native,
    Fake(Arc<dyn FileSystem + Send + Sync>),
}

/// How big the browser opens, and how small it may be dragged.
const BROWSER_SIZE: egui::Vec2 = egui::vec2(720.0, 460.0);
const BROWSER_MIN_SIZE: egui::Vec2 = egui::vec2(420.0, 300.0);

/// How much clear space the browser's buttons keep above and below their text. Four points
/// less than the page's, which is what takes the top strip down from towering to trim.
const BROWSER_BUTTON_PADDING_Y: f32 = 2.0;

/// The page's style with the scroll bars made solid - see [`Browsing::update`].
fn browser_style(outer: &egui::Style) -> egui::Style {
    let mut style = outer.clone();
    // The crate sizes its whole top strip off the *body text height plus this padding*, so
    // this is the one lever over how tall that strip stands. The page's own padding is set
    // for buttons a player aims at; the browser's top row is dense furniture - navigation
    // arrows, a breadcrumb, a menu - and at the page's padding it towers over its contents.
    style.spacing.button_padding.y = BROWSER_BUTTON_PADDING_Y;
    style.spacing.scroll = egui::style::ScrollStyle {
        // Thinner than the page's bar: this one lives inside a one-line breadcrumb, where
        // every point it takes is a point the box grows by.
        bar_width: outer.spacing.scroll.bar_width * 0.8,
        bar_inner_margin: 2.0,
        // `solid` reserves the strip but leaves the bar invisible until the pointer is over
        // it, which for a one-line breadcrumb means a strip of nothing and no sign that the
        // path scrolls at all. Shown at rest instead - and drawn in the foreground colour,
        // because the handle's own fill is the same sunken navy as the trough it sits in.
        foreground_color: true,
        dormant_background_opacity: 0.25,
        dormant_handle_opacity: 0.35,
        active_handle_opacity: 0.6,
        ..egui::style::ScrollStyle::solid()
    };
    style
}

/// The browser's title bar - which folder this pick is for, in the same words as the field.
fn title(field: BrowseField) -> &'static str {
    match field {
        BrowseField::GameInstallation => "Find the Civilization V game folder",
        BrowseField::Documents => "Find the Civilization 5 Documents folder",
        BrowseField::DevCheckout => "Find the Community-Patch-DLL folder",
    }
}
