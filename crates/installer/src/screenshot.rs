//! `--screenshot <dir>`: render every screen to PNG with no user present.
//!
//! This is half of how UI work is verified (rule 15). The other half is the `egui_kittest`
//! snapshot baselines; both drive the same [`InstallerApp::show`], so what lands in a PNG
//! here is what the shipped window draws.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::app::{InstallerApp, Screen};
use crate::cli::ScreenshotOptions;

#[derive(Debug, Clone, Copy)]
struct Job {
    screen: Screen,
    size: [f32; 2],
    scale: f32,
}

/// Render every screen at every requested size and scale, then exit.
pub fn run(options: &ScreenshotOptions) -> Result<(), String> {
    std::fs::create_dir_all(&options.directory)
        .map_err(|err| format!("could not create {}: {err}", options.directory.display()))?;

    let jobs: Vec<Job> = options
        .sizes
        .iter()
        .flat_map(|size| {
            options.scales.iter().flat_map(move |scale| {
                Screen::ALL.into_iter().map(move |screen| Job {
                    screen,
                    size: *size,
                    scale: *scale,
                })
            })
        })
        .collect();

    let Some(first) = jobs.first().copied() else {
        return Err("nothing to render".to_owned());
    };

    let problems = Arc::new(Mutex::new(Vec::<String>::new()));
    let run = ScreenshotRun {
        directory: options.directory.clone(),
        jobs,
        index: 0,
        preview: None,
        phase: Phase::SettingScale,
        frames_in_phase: 0,
        problems: Arc::clone(&problems),
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Civ 5 VP Installer — rendering screenshots")
            .with_inner_size(first.size)
            // No window is shown: this runs unattended, including from a script.
            .with_visible(false),
        ..Default::default()
    };

    eframe::run_native(
        "civ5vp-installer-screenshots",
        native_options,
        Box::new(move |_cc| Ok(Box::new(run))),
    )
    .map_err(|err| format!("could not open a render surface: {err}"))?;

    let problems = problems
        .lock()
        .map_err(|_| "render thread panicked".to_owned())?;
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// A job is set up one step at a time, because each step only takes effect on a later frame
/// — and because the order matters: `InnerSize` is given in points, so it is interpreted
/// against whatever the DPI scale happens to be when it arrives. Scale first, then size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    SettingScale,
    SettingSize,
    Shooting,
}

struct ScreenshotRun {
    directory: PathBuf,
    jobs: Vec<Job>,
    index: usize,
    /// The app frozen in the current job's screen. `None` means "start the next job".
    preview: Option<InstallerApp>,
    phase: Phase,
    frames_in_phase: u32,
    problems: Arc<Mutex<Vec<String>>>,
}

/// Frames to wait for a resize/rescale to take effect before giving up and moving on.
const SETTLE_LIMIT: u32 = 120;

impl ScreenshotRun {
    fn note_problem(&self, problem: String) {
        if let Ok(mut problems) = self.problems.lock() {
            problems.push(problem);
        }
    }

    fn finish_job(&mut self) {
        self.index += 1;
        self.preview = None;
    }
}

impl eframe::App for ScreenshotRun {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Nobody is going to move a mouse to produce the next frame.
        ctx.request_repaint();

        let Some(job) = self.jobs.get(self.index).copied() else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };

        if self.preview.is_none() {
            self.preview = Some(InstallerApp::preview(job.screen));
            ctx.set_pixels_per_point(job.scale);
            self.phase = Phase::SettingScale;
            self.frames_in_phase = 0;
        }
        self.frames_in_phase += 1;

        if let Some(preview) = &mut self.preview {
            preview.show(ui);
        }

        let delivered = ctx.input(|input| {
            input.raw.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });

        if let Some(rendered) = delivered {
            let path = self.directory.join(file_name(&job));
            match save_png(&rendered, &path) {
                Ok(()) => println!("wrote {}", path.display()),
                Err(err) => self.note_problem(err),
            }
            self.finish_job();
            return;
        }

        let timed_out = self.frames_in_phase > SETTLE_LIMIT;
        match self.phase {
            Phase::SettingScale => {
                if (ctx.pixels_per_point() - job.scale).abs() < 0.001 {
                    // Only now is a size in points worth anything.
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        job.size[0],
                        job.size[1],
                    )));
                    self.phase = Phase::SettingSize;
                    self.frames_in_phase = 0;
                } else if timed_out {
                    self.note_problem(format!(
                        "{}: asked for a DPI scale of {} but got {}",
                        file_name(&job),
                        job.scale,
                        ctx.pixels_per_point(),
                    ));
                    self.finish_job();
                }
            }
            Phase::SettingSize => {
                let viewport = ctx.viewport_rect();
                let settled = (viewport.width() - job.size[0]).abs() < 1.0
                    && (viewport.height() - job.size[1]).abs() < 1.0;
                if settled && self.frames_in_phase > 1 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                    self.phase = Phase::Shooting;
                    self.frames_in_phase = 0;
                } else if timed_out {
                    // Writing a PNG whose name promises a size it does not have would be
                    // worse than writing nothing, so this job is abandoned and reported.
                    self.note_problem(format!(
                        "{}: the window manager gave {}x{} points instead of {}x{}; \
                         no file written",
                        file_name(&job),
                        viewport.width(),
                        viewport.height(),
                        job.size[0],
                        job.size[1],
                    ));
                    self.finish_job();
                }
            }
            Phase::Shooting => {
                if timed_out {
                    self.note_problem(format!(
                        "{}: no screenshot came back from the renderer",
                        file_name(&job),
                    ));
                    self.finish_job();
                }
            }
        }
    }
}

fn file_name(job: &Job) -> String {
    format!(
        "{}-{}x{}@{}x.png",
        job.screen.file_stem(),
        job.size[0] as u32,
        job.size[1] as u32,
        job.scale,
    )
}

fn save_png(rendered: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(rendered.pixels.len() * 4);
    for pixel in &rendered.pixels {
        bytes.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    image::save_buffer(
        path,
        &bytes,
        rendered.width() as u32,
        rendered.height() as u32,
        image::ColorType::Rgba8,
    )
    .map_err(|err| format!("could not write {}: {err}", path.display()))
}
