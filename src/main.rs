#[cfg(not(all(windows, target_arch = "x86_64")))]
compile_error!("XFold currently supports Windows x64.");

mod desktop;
mod mkv;
mod runner;
mod tools;
mod web;
mod windows;

use std::{ffi::OsString, path::PathBuf, process::ExitCode};

const USAGE: &str = "Usage:
  xfold [--state-dir DIRECTORY] convert <dts-hd-ma|true-hd> <INPUT.dts> <OUTPUT>
  xfold [--state-dir DIRECTORY] convert <dts-hd-ma|true-hd> --track ID <INPUT.mkv> <OUTPUT>
  xfold [--state-dir DIRECTORY] convert <dts-hd-ma|true-hd> --track ID --mkv-mode <add|replace> <INPUT.mkv> <OUTPUT.mkv>
  xfold [--state-dir DIRECTORY] tracks <INPUT.mkv>
  xfold [--state-dir DIRECTORY] status
  xfold [--state-dir DIRECTORY] serve [--no-browser]

Press Ctrl+C during conversion to cancel. Use .dts for DTS-HD MA, .thd for TrueHD,
or .mka for standalone Matroska audio.";

fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let state = if args.first().is_some_and(|arg| arg == "--state-dir") {
        if args.len() < 2 {
            return Err(USAGE.into());
        }
        args.remove(0);
        Some(PathBuf::from(args.remove(0)))
    } else {
        None
    };
    match args.as_slice() {
        [] | [_] if args.is_empty() || args[0] == "serve" => {
            web::serve(state.map_or_else(runner::default_state, Ok)?, true)?;
        }
        [command, option] if command == "serve" && option == "--no-browser" => {
            web::serve(state.map_or_else(runner::default_state, Ok)?, false)?;
        }
        [arg] if arg == "--help" || arg == "-h" => println!("{USAGE}"),
        [arg] if arg == "--version" => println!("xfold {}", env!("CARGO_PKG_VERSION")),
        [arg] if arg == "status" => {
            let state = state.map_or_else(runner::default_state, Ok)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&runner::status(&state)?)?
            );
        }
        [command, input] if command == "tracks" => {
            let state = state.map_or_else(runner::default_state, Ok)?;
            windows::install_console_handler()?;
            let tracks = runner::list_tracks(&state, input.as_ref(), &windows::CANCELLED)?;
            println!("{}", serde_json::to_string_pretty(&tracks)?);
        }
        [command, preset, rest @ ..] if command == "convert" => {
            let preset = match preset.to_str() {
                Some("dts-hd-ma") => runner::Preset::DtsHdMa,
                Some("true-hd") => runner::Preset::TrueHd,
                _ => return Err(USAGE.into()),
            };
            let parse_id = |id: &OsString| {
                id.to_str()
                    .and_then(|value| value.parse::<u32>().ok())
                    .ok_or("Use a numeric track ID from the tracks command.")
            };
            let (input, output, track, mkv_mode) = match rest {
                [input, output] => (input, output, None, None),
                [option, id, input, output] if option == "--track" => {
                    (input, output, Some(parse_id(id)?), None)
                }
                [option, id, mode_option, mode, input, output]
                    if option == "--track" && mode_option == "--mkv-mode" =>
                {
                    let mode = match mode.to_str() {
                        Some("add") => mkv::MkvMode::Add,
                        Some("replace") => mkv::MkvMode::Replace,
                        _ => return Err("Use --mkv-mode add or replace.".into()),
                    };
                    (input, output, Some(parse_id(id)?), Some(mode))
                }
                _ => return Err(USAGE.into()),
            };
            let state = state.map_or_else(runner::default_state, Ok)?;
            windows::install_console_handler()?;
            let attempt = runner::convert(
                &state,
                input.as_ref(),
                output.as_ref(),
                preset,
                track,
                mkv_mode,
                &windows::CANCELLED,
            )?;
            println!("{}", serde_json::to_string_pretty(&attempt)?);
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xfold: {error}");
            if windows::CANCELLED.load(std::sync::atomic::Ordering::Relaxed) {
                ExitCode::from(130)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
