use crate::{desktop, runner::Preset, windows};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
};

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Theme {
    System,
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub theme: Theme,
    pub preset: Preset,
    pub mkv_mka: bool,
    pub port: u16,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            preset: Preset::DtsHdMa,
            mkv_mka: true,
            port: 0,
        }
    }
}

impl Preferences {
    pub fn load(dir: &Path) -> super::Result<Self> {
        match File::open(dir.join("settings.json")) {
            Ok(file) => Ok(serde_json::from_reader(file)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self, dir: &Path) -> super::Result<()> {
        let temp = dir.join(format!("settings-{}.tmp", desktop::token()?));
        let result = (|| -> super::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            serde_json::to_writer_pretty(&mut file, self)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            windows::rename(&temp, &dir.join("settings.json"), true)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }

    pub fn change(&mut self, key: &str, value: &str) -> bool {
        match (key, value) {
            ("theme", "system") => self.theme = Theme::System,
            ("theme", "light") => self.theme = Theme::Light,
            ("theme", "dark") => self.theme = Theme::Dark,
            ("preset", "dts-hd-ma") => self.preset = Preset::DtsHdMa,
            ("preset", "true-hd") => self.preset = Preset::TrueHd,
            ("mkv-format", "mka") => self.mkv_mka = true,
            ("mkv-format", "raw") => self.mkv_mka = false,
            ("port", value) => match value.parse::<u16>() {
                Ok(port) => self.port = port,
                Err(_) => return false,
            },
            _ => return false,
        }
        true
    }
}
