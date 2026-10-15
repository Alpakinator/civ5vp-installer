//! The Civ 5 VP Installer's egui shell.
//!
//! The whole of the installer's behaviour lives in `civ5vp-core`; this crate opens a window
//! and draws what the Core reports. It is a library as well as a binary so that the
//! `egui_kittest` harness can drive the real UI through its AccessKit tree.

// No panicking paths in code reachable from the UI. `main.rs` is a separate crate root and
// keeps the latitude startup wiring needs.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

pub mod app;
pub mod browse;
pub mod cli;
pub mod deco;
pub mod placeholder;
pub mod reveal;
pub mod screenshot;
pub mod theme;
pub mod update;
pub mod wiring;

pub use app::{InstallerApp, Screen};

/// Where the log file lives once [`init_log_file`] has run - inside the App Data Store.
static LOG_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Point the log at its real file. Called once at startup with the App Data
/// Store resolved; until then - and always, additionally - detail goes to stderr.
pub fn init_log_file(path: std::path::PathBuf) {
    let _ = LOG_FILE.set(path);
}

/// The log file's path, for the "Open log" button and the failure panel.
pub fn log_file() -> Option<&'static std::path::Path> {
    LOG_FILE.get().map(std::path::PathBuf::as_path)
}

/// Show `path` to the user with the platform's opener - the "Open log" button.
///
/// This is rule 5's second permitted exception (see CODING_STANDARDS.md): a best-effort
/// convenience that invokes the desktop's own opener, never anything the user must install
/// for the installer to work. If it fails, the path is on screen to copy and the log has the
/// reason - no install is ever affected.
pub fn open_path(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let opener = "xdg-open";
    if let Err(err) = std::process::Command::new(opener).arg(path).spawn() {
        log_detail(&format!(
            "could not open {} with {opener}: {err}",
            path.display()
        ));
    }
}

/// The window settings the installer and the `--screenshot` renderer share.
///
/// eframe asks wgpu for the high-performance GPU, which on a laptop with two is the discrete
/// one - not the one driving the screen, so every frame is copied across to be shown, and the
/// discrete GPU powers down whenever the window stops drawing. When it then fails to wake (an
/// RTX 3050 under NVIDIA's 610 driver was seen to, with Xid 119), the frame in flight never
/// finishes, the compositor never asks for another, and the window shows its last frame for
/// good while the install carries on unseen behind it. The installer draws flat shapes and
/// text; the GPU that is already awake is the right one. `WGPU_POWER_PREF=high` still
/// overrides this, as it does for any wgpu program.
pub fn native_options(viewport: egui::ViewportBuilder) -> eframe::NativeOptions {
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.power_preference = eframe::wgpu::PowerPreference::from_env()
            .unwrap_or(eframe::wgpu::PowerPreference::LowPower);
    }
    options
}

/// Where the detail behind a user-facing error goes: appended to the log file in the App
/// Data Store (and echoed to stderr), keeping it out of the UI. A log line that cannot be
/// written is not worth interrupting anything over - stderr still has it.
pub fn log_detail(detail: &str) {
    if let Some(path) = LOG_FILE.get() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let line = format!("[{timestamp}] {detail}\n");
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
        if written.is_err() {
            eprintln!("[civ5vp-installer] could not write {}", path.display());
        }
    }
    eprintln!("[civ5vp-installer] {detail}");
}
