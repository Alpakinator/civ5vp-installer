//! The Civ 5 VP Installer's egui shell.
//!
//! The whole of the installer's behaviour lives in `civ5vp-core`; this crate opens a window
//! and draws what the Core reports (rule 3). It is a library as well as a binary so that the
//! `egui_kittest` harness can drive the real UI through its AccessKit tree.

pub mod app;
pub mod cli;
pub mod placeholder;
pub mod screenshot;

pub use app::{InstallerApp, Screen, Status};

/// Where the detail behind a user-facing error goes.
///
/// Rule 11 wants everything a user might report in a log file, and rule 10 keeps that detail
/// out of the UI. Ticket 10 gives this a real file with copy/open buttons; until then it goes
/// to stderr, which is at least somewhere a developer can look.
pub fn log_detail(detail: &str) {
    eprintln!("[civ5vp-installer] {detail}");
}
