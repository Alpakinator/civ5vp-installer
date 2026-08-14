//! Command-line arguments.
//!
//! Hand-rolled rather than pulled from a crate: there are three options, and rule 17 asks
//! for a reason before every dependency.

use std::path::PathBuf;

pub const USAGE: &str = "\
Civ 5 VP Installer

USAGE:
    civ5vp-installer                          Run the installer window
    civ5vp-installer --screenshot <dir>       Render every screen to PNG and exit

OPTIONS:
    --screenshot <dir>   Directory to write the PNGs into (created if missing)
    --size <WxH>         Window size in points, e.g. 900x860. Repeat for several sizes.
                         Default: 900x860
    --scale <factor>     DPI scale, e.g. 1.5. Repeat for several scales. Default: 1
    -h, --help           Show this message
";

/// The default window size, in points.
pub const DEFAULT_SIZE: [f32; 2] = [900.0, 860.0];

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotOptions {
    pub directory: PathBuf,
    /// Sizes in points. Every screen is rendered at every size and every scale.
    pub sizes: Vec<[f32; 2]>,
    pub scales: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    RunApp,
    Screenshot(ScreenshotOptions),
    Help,
}

/// Parse arguments, excluding the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut directory: Option<PathBuf> = None;
    let mut sizes: Vec<[f32; 2]> = Vec::new();
    let mut scales: Vec<f32> = Vec::new();

    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--screenshot" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--screenshot needs a directory".to_owned())?;
                directory = Some(PathBuf::from(value));
            }
            "--size" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--size needs a value like 900x860".to_owned())?;
                sizes.push(parse_size(&value)?);
            }
            "--scale" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--scale needs a value like 1.5".to_owned())?;
                scales.push(parse_scale(&value)?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    match directory {
        Some(directory) => Ok(Command::Screenshot(ScreenshotOptions {
            directory,
            sizes: if sizes.is_empty() {
                vec![DEFAULT_SIZE]
            } else {
                sizes
            },
            scales: if scales.is_empty() { vec![1.0] } else { scales },
        })),
        None if !sizes.is_empty() || !scales.is_empty() => {
            Err("--size and --scale only apply to --screenshot".to_owned())
        }
        None => Ok(Command::RunApp),
    }
}

fn parse_size(value: &str) -> Result<[f32; 2], String> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected a size like 900x860, got {value}"))?;
    let width: f32 = width
        .trim()
        .parse()
        .map_err(|_| format!("bad width in {value}"))?;
    let height: f32 = height
        .trim()
        .parse()
        .map_err(|_| format!("bad height in {value}"))?;
    if width <= 0.0 || height <= 0.0 {
        return Err(format!("size must be positive, got {value}"));
    }
    Ok([width, height])
}

fn parse_scale(value: &str) -> Result<f32, String> {
    let scale: f32 = value
        .trim()
        .parse()
        .map_err(|_| format!("expected a number like 1.5, got {value}"))?;
    if scale <= 0.0 {
        return Err(format!("scale must be positive, got {value}"));
    }
    Ok(scale)
}
