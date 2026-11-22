use serde_json::Value;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use crate::runner::Preset;

pub struct Invocation {
    pub exe: PathBuf,
    pub args: Vec<OsString>,
}

pub fn bundle_root() -> io::Result<PathBuf> {
    let host = std::env::current_exe()?;
    host.ancestors()
        .skip(1)
        .find_map(|dir| {
            let bundle = dir.join("tools/bin");
            (dir.join("Cargo.toml").is_file() || host.parent() == Some(dir))
                .then_some(bundle)
                .filter(|p| p.is_dir())
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cannot locate app-private tools/bin beside xfold.exe or at the development root",
            )
        })
}

pub struct Tools {
    harletty: PathBuf,
    harlettizer: PathBuf,
}

impl Tools {
    pub fn resolve() -> io::Result<Self> {
        let root = bundle_root()?;
        let resolve = |relative| root.join(relative).canonicalize();
        Ok(Self {
            harletty: resolve("harletty/harletty.exe")?,
            harlettizer: resolve("harlettizer/harlettizer.exe")?,
        })
    }

    pub fn info(&self, path: &Path, config: &Path, codec: &str) -> Invocation {
        Invocation {
            exe: self.harletty.clone(),
            args: vec![
                "info".into(),
                "--codec".into(),
                codec.into(),
                "--strict".into(),
                "--json".into(),
                "--max-seconds".into(),
                "20".into(),
                "--config".into(),
                config.into(),
                "--".into(),
                path.into(),
            ],
        }
    }

    pub fn decode(&self, input: &Path, master_base: &Path, config: &Path) -> Invocation {
        Invocation {
            exe: self.harletty.clone(),
            args: vec![
                "decode".into(),
                "--codec".into(),
                "dts".into(),
                "--strict".into(),
                "--bed-conform".into(),
                "--output-path".into(),
                master_base.into(),
                "--config".into(),
                config.into(),
                "--".into(),
                input.into(),
            ],
        }
    }

    pub fn verify_decode(
        &self,
        output: &Path,
        scratch: &Path,
        config: &Path,
        codec: &str,
    ) -> Invocation {
        Invocation {
            exe: self.harletty.clone(),
            args: vec![
                "decode".into(),
                "--codec".into(),
                codec.into(),
                "--strict".into(),
                "--no-audio".into(),
                "--output-path".into(),
                scratch.into(),
                "--config".into(),
                config.into(),
                "--".into(),
                output.into(),
            ],
        }
    }

    pub fn encode(
        &self,
        preset: Preset,
        master: &Path,
        output: &Path,
        config: &Path,
    ) -> Invocation {
        let mut args: Vec<OsString> = vec!["--config".into(), config.into()];
        let options: &[&str] = match preset {
            Preset::DtsHdMa => &["dts", "--layout", "7.1", "--bits", "24"],
            Preset::TrueHd => &[
                "encode",
                "--cluster",
                "12",
                "--fold-depth",
                "24",
                "--presentations",
            ],
        };
        args.extend(options.iter().map(OsString::from));
        args.extend([
            "--progress".into(),
            "--out".into(),
            output.into(),
            "--".into(),
            master.into(),
        ]);
        Invocation {
            exe: self.harlettizer.clone(),
            args,
        }
    }
}

pub fn has_complete_truehd_presentations(summary: &str) -> bool {
    summary.lines().any(|line| {
        line.trim_start()
            .strip_prefix("presentations the folds themselves, in every one of ")
            .and_then(|line| line.strip_suffix(" restart intervals"))
            .and_then(|count| count.parse::<u64>().ok())
            .is_some_and(|count| count > 0)
    })
}

pub fn truehd_decoded_samples(log: &str) -> Option<u64> {
    log.lines().rev().find_map(|line| {
        let (_, summary) = line.split_once("Processing complete: ")?;
        let (_, samples) = summary.split_once(" frames, ")?;
        samples.strip_suffix(" samples")?.parse().ok()
    })
}

pub fn validate_input(info: &Value) -> bool {
    info["schema"] == 1
        && info["sample_rate"] == 48000
        && info["spatial"]["kind"] == "dtsx"
        && info["frames_seen"]
            .as_u64()
            .is_some_and(|frames| frames > 0)
}

pub fn validate_output(info: &Value, preset: Preset) -> bool {
    info["schema"] == 1
        && info["sample_rate"] == 48000
        && info["frames_seen"]
            .as_u64()
            .is_some_and(|frames| frames > 0)
        && match preset {
            Preset::DtsHdMa => {
                info["codec"] == "DTS-HD MA" && info["channels"] == 8 && info["spatial"].is_null()
            }
            Preset::TrueHd => {
                info["codec"] == "TrueHD"
                    && info["truehd"]["atmos"] == true
                    && info["spatial"]["kind"] == "atmos"
            }
        }
}
