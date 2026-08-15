//! The binary's command line, exercised through its public parser.

use std::path::PathBuf;

use civ5vp_installer::cli::{self, Command, DEFAULT_SIZE};

fn parse(args: &[&str]) -> Result<Command, String> {
    cli::parse(args.iter().map(|arg| (*arg).to_owned()))
}

#[test]
fn no_arguments_runs_the_window() {
    assert_eq!(parse(&[]), Ok(Command::RunApp));
}

#[test]
fn screenshot_defaults_to_one_size_and_scale() {
    let Ok(Command::Screenshot(options)) = parse(&["--screenshot", "shots"]) else {
        panic!("expected a screenshot command");
    };
    assert_eq!(options.directory, PathBuf::from("shots"));
    assert_eq!(options.sizes, vec![DEFAULT_SIZE]);
    assert_eq!(options.scales, vec![1.0]);
}

#[test]
fn sizes_and_scales_can_be_repeated() {
    let Ok(Command::Screenshot(options)) = parse(&[
        "--screenshot",
        "shots",
        "--size",
        "800x600",
        "--size",
        "1280x800",
        "--scale",
        "1",
        "--scale",
        "1.5",
    ]) else {
        panic!("expected a screenshot command");
    };
    assert_eq!(options.sizes, vec![[800.0, 600.0], [1280.0, 800.0]]);
    assert_eq!(options.scales, vec![1.0, 1.5]);
}

#[test]
fn bad_input_is_rejected_with_a_reason() {
    assert!(parse(&["--screenshot"]).is_err());
    assert!(parse(&["--screenshot", "shots", "--size", "wide"]).is_err());
    assert!(parse(&["--screenshot", "shots", "--scale", "0"]).is_err());
    assert!(parse(&["--wat"]).is_err());
    // Size and scale mean nothing without --screenshot; saying so beats ignoring them.
    assert!(parse(&["--size", "800x600"]).is_err());
}

#[test]
fn help_is_asked_for_by_either_spelling() {
    assert_eq!(parse(&["-h"]), Ok(Command::Help));
    assert_eq!(parse(&["--help"]), Ok(Command::Help));
}
