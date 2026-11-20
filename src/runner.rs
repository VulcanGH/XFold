use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    mkv::{Identification, MkvMode, MkvTools, Track},
    tools::{self, Invocation, Tools},
    windows,
};

mod remux;

const MIB: u64 = 1024 * 1024;
const RESERVE: u64 = 16 * MIB;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    DtsHdMa,
    TrueHd,
}

impl Preset {
    pub fn extension(self) -> &'static str {
        match self {
            Self::DtsHdMa => "dts",
            Self::TrueHd => "thd",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    Preflight,
    Inspect,
    Extract,
    Decode,
    Encode,
    Mux,
    Verify,
    Publishing,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl Stage {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Preflight
                | Self::Inspect
                | Self::Extract
                | Self::Decode
                | Self::Encode
                | Self::Mux
                | Self::Verify
                | Self::Publishing
        )
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Attempt {
    id: String,
    input: PathBuf,
    output: PathBuf,
    preset: Preset,
    #[serde(skip_serializing_if = "Option::is_none")]
    track_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mkv_mode: Option<MkvMode>,
    stage: Stage,
    started_unix_ms: u128,
    updated_unix_ms: u128,
    detail: Option<String>,
}

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

impl Attempt {
    pub fn active(&self) -> bool {
        self.stage.active()
    }

    pub fn progress(&self, state: &Path) -> Option<u8> {
        if self.stage != Stage::Encode {
            return None;
        }
        let log = self.log_path(state)?.join("encode.stderr.txt");
        tail(&log).ok()?.lines().rev().find_map(|line| {
            line.strip_prefix("progress ")?
                .strip_suffix('%')?
                .parse::<u8>()
                .ok()
                .filter(|&percent| percent <= 100)
        })
    }

    fn log_path(&self, state: &Path) -> Option<PathBuf> {
        if self.id.is_empty() || !self.id.bytes().all(|ch| ch.is_ascii_digit() || ch == b'-') {
            return None;
        }
        Some(state.join("logs").join(&self.id))
    }

    pub fn diagnostics(&self, state: &Path) -> Vec<(&'static str, String)> {
        let Some(logs) = self.log_path(state) else {
            return Vec::new();
        };
        [
            "identify-source.stdout.txt",
            "extract-source.stdout.txt",
            "mux-audio.stdout.txt",
            "remux.stdout.txt",
            "inspect.stderr.txt",
            "decode.stderr.txt",
            "encode.stdout.txt",
            "encode.stderr.txt",
            "verify.stderr.txt",
            "verify-decode.stderr.txt",
        ]
        .into_iter()
        .filter_map(|name| {
            let content = tail(&logs.join(name)).ok()?;
            (!content.trim().is_empty()).then_some((name, content))
        })
        .collect()
    }

    fn workspace(&self) -> Result<PathBuf> {
        let parent = self.output.parent().ok_or("output has no parent")?;
        if !parent.is_absolute()
            || self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
        {
            return Err("invalid attempt workspace path".into());
        }
        Ok(parent.join(format!(".xfold-{}", self.id)))
    }

    fn set(&mut self, state: &Path, stage: Stage, detail: Option<String>) -> Result<()> {
        self.stage = stage;
        self.detail = detail;
        self.updated_unix_ms = now();
        let pending = state.join("attempt.pending");
        let final_path = state.join("attempt.json");
        // Refuse to follow a preexisting link when creating the new snapshot.
        match fs::remove_file(&pending) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        // A status reader can briefly hold the old snapshot open on Windows.
        for retry in 0..20 {
            match windows::rename(&pending, &final_path, true) {
                Ok(()) => return Ok(()),
                Err(error) if retry < 19 && matches!(error.raw_os_error(), Some(5 | 32)) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(format!("Could not save attempt status: {error}").into()),
            }
        }
        unreachable!()
    }
}

pub fn default_state() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .ok_or("LOCALAPPDATA is unavailable; supply --state-dir")?,
    )
    .join("XFold"))
}

fn lock(state: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(state.join("runner.lock"))
}

fn load(state: &Path) -> Result<Option<Attempt>> {
    match File::open(state.join("attempt.json")) {
        Ok(file) => Ok(Some(serde_json::from_reader(file)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn clean(work: &Path) -> io::Result<()> {
    // The workspace name is reserved per attempt. Its ownership marker is
    // written only after create_dir succeeds.
    if !work.join("xfold-workspace").is_file() {
        return Ok(());
    }
    for attempt in 0..10 {
        match fs::remove_dir_all(work) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) if attempt == 9 => return Err(e),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
    unreachable!()
}

pub fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

fn open_source(input: &Path) -> Result<File> {
    // Keep readers working and disallow writers and deletion during the attempt.
    let source = OpenOptions::new().read(true).share_mode(1).open(input)?;
    let metadata = source.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("Select a non-empty input file.".into());
    }
    Ok(source)
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled. Run the command again to retry.".into());
    }
    Ok(())
}

fn run_tool(
    work: &Path,
    logs: &Path,
    name: &str,
    command: Invocation,
    monitor: impl Fn() -> Result<()>,
) -> Result<PathBuf> {
    monitor()?;
    let stdout_path = logs.join(format!("{name}.stdout.txt"));
    let stderr_path = logs.join(format!("{name}.stderr.txt"));
    let stdout = File::create_new(&stdout_path)?;
    let stderr = File::create_new(&stderr_path)?;
    let child = windows::Child::spawn(&command.exe, &command.args, work, &stdout, &stderr)
        .map_err(|error| format!("Could not launch {}: {error}", command.exe.display()))?;
    loop {
        if let Err(error) = monitor() {
            if let Err(termination) = child.terminate() {
                return Err(format!("{error} Process cleanup: {termination}").into());
            }
            return Err(error);
        }
        if let Some(code) = child.poll(100)? {
            drop(child);
            if code != 0 {
                let diagnostic = tail(&stderr_path).unwrap_or_default();
                let diagnostic = if diagnostic.trim().is_empty() {
                    tail(&stdout_path).unwrap_or_default()
                } else {
                    diagnostic
                };
                return Err(format!(
                    "{name} failed (exit {code}). Check the input and free space. {}",
                    diagnostic.trim(),
                )
                .into());
            }
            return Ok(stdout_path);
        }
    }
}

pub fn list_tracks(state: &Path, input: &Path, cancel: &AtomicBool) -> Result<Vec<Track>> {
    let input = input.canonicalize()?;
    let _source = open_source(&input)?;
    let mkv = MkvTools::resolve()?;
    fs::create_dir_all(state)?;
    let scratch = state
        .canonicalize()?
        .join(format!("tracks-{}-{}", now(), std::process::id()));
    fs::create_dir(&scratch)?;
    File::create_new(scratch.join("xfold-workspace"))?;
    let result = run_tool(&scratch, &scratch, "identify", mkv.identify(&input), || {
        check_cancel(cancel)
    })
    .and_then(|report| read_matroska(&report))
    .map(Identification::audio_tracks);
    let cleanup = clean(&scratch);
    let tracks = result?;
    cleanup?;
    Ok(tracks)
}

fn recover(state: &Path, attempt: &mut Attempt) -> Result<()> {
    if attempt.stage.active() {
        let publishing = attempt.stage == Stage::Publishing;
        let mut detail = "The previous XFold process exited during this attempt.".to_string();
        if publishing {
            detail.push_str(
                " Check the destination before retrying. Publication may already have completed.",
            );
        }
        if let Err(error) = clean(&attempt.workspace()?) {
            detail.push_str(&format!(" Could not remove the workspace: {error}."));
        }
        attempt.set(state, Stage::Interrupted, Some(detail))?;
    }
    Ok(())
}

pub fn status(state: &Path) -> Result<Option<Attempt>> {
    let latest = load(state)?;
    if !latest
        .as_ref()
        .is_some_and(|attempt| attempt.stage.active())
    {
        return Ok(latest);
    }
    match lock(state) {
        Ok(_lock) => {
            let mut latest = load(state)?;
            if let Some(attempt) = &mut latest {
                recover(state, attempt)?;
            }
            Ok(latest)
        }
        Err(error) if error.raw_os_error() == Some(32) => load(state),
        Err(error) => Err(error.into()),
    }
}

struct Job<'a> {
    state: &'a Path,
    work: PathBuf,
    logs: PathBuf,
    cancel: &'a AtomicBool,
    attempt: &'a mut Attempt,
    owns_work: bool,
}

impl Job<'_> {
    fn check_cancel(&self) -> Result<()> {
        check_cancel(self.cancel)
    }

    fn space(&self, minimum: u64) -> Result<()> {
        let volume = self.attempt.output.parent().ok_or("output has no parent")?;
        let available = windows::free_space(volume)?;
        require_space(available, minimum)
    }

    fn step(&mut self, stage: Stage, name: &str, command: Invocation) -> Result<PathBuf> {
        self.check_cancel()?;
        self.attempt.set(self.state, stage, None)?;
        run_tool(&self.work, &self.logs, name, command, || {
            self.check_cancel().and_then(|()| self.space(RESERVE))
        })
        .map_err(|error| format!("{error}\nLogs: {}", self.logs.display()).into())
    }

    fn identify_source(&mut self, mkv: &MkvTools, input: &Path, id: u32) -> Result<Identification> {
        let report = self.step(Stage::Inspect, "identify-source", mkv.identify(input))?;
        let identified = read_matroska(&report)?;
        let track = identified
            .tracks
            .iter()
            .find(|track| track.id == id)
            .ok_or_else(|| {
                format!("Track ID {id} is absent. Run tracks to find the current track IDs.")
            })?;
        if !track.has_codec("A_DTS") {
            return Err(format!(
                "Track ID {id} is {}. Select a DTS:X audio track.",
                track.codec
            )
            .into());
        }
        Ok(identified)
    }

    fn mux_audio(&mut self, mkv: &MkvTools, raw: &Path, preset: Preset) -> Result<PathBuf> {
        // The raw stream stays on disk while writing the MKA and extracting its
        // track for byte-for-byte verification before publication.
        let raw_size = fs::metadata(raw)?.len();
        self.space(RESERVE.saturating_add(raw_size.saturating_mul(3)))?;
        let container = self.work.join("export.mka");
        self.step(Stage::Mux, "mux-audio", mkv.mux_audio(raw, &container))?;
        let report = self.step(Stage::Verify, "identify-export", mkv.identify(&container))?;
        let identified = read_matroska(&report)?;
        let codec_id = match preset {
            Preset::DtsHdMa => "A_DTS",
            Preset::TrueHd => "A_TRUEHD",
        };
        let [track] = identified.tracks.as_slice() else {
            return Err("MKA verification expected exactly one audio track.".into());
        };
        if !track.has_codec(codec_id) {
            return Err(format!("MKA verification expected a {codec_id} audio track.").into());
        }
        let extracted = self.work.join(format!("verified.{}", preset.extension()));
        self.step(
            Stage::Verify,
            "extract-export",
            mkv.extract(&container, track.id, &extracted),
        )?;
        compare_payload(raw, &extracted, self.cancel)?;
        Ok(container)
    }

    fn convert(&mut self, tools: &Tools) -> Result<Option<String>> {
        self.check_cancel()?;
        let input = self.attempt.input.clone();
        let output = self.attempt.output.clone();
        let preset = self.attempt.preset;
        if output.symlink_metadata().is_ok() {
            return Err("Destination already exists. Choose a new file name.".into());
        }
        let source = open_source(&input)?;
        let track_id = self.attempt.track_id;
        let mka = has_extension(&output, "mka");
        let mkv = (track_id.is_some() || mka)
            .then(MkvTools::resolve)
            .transpose()
            .map_err(|error| format!("Cannot resolve the staged MKVToolNix tools: {error}"))?;
        let source_size = source.metadata()?.len();
        let raw_estimate = if track_id.is_none() {
            source_size.saturating_mul(8)
        } else {
            0
        };
        self.space(RESERVE.saturating_add(raw_estimate))?;
        fs::create_dir(&self.work)?;
        self.owns_work = true;
        File::create_new(self.work.join("xfold-workspace"))?;
        let config = self.work.join("settings.yaml");
        fs::write(&config, "{}\n")?;

        let mut remux = None;
        let audio_input = if let (Some(mkv), Some(id)) = (&mkv, track_id) {
            let report = self.identify_source(mkv, &input, id)?;
            if let Some(mode) = self.attempt.mkv_mode {
                remux = Some(self.plan_remux(mkv, &input, report, id, mode)?);
            }
            let extracted = self.work.join("source.dts");
            self.step(
                Stage::Extract,
                "extract-source",
                mkv.extract(&input, id, &extracted),
            )?;
            extracted
        } else {
            input.clone()
        };
        // Estimate from the compressed audio size, excluding any MKV video or
        // other tracks. Use actual master bytes after decoding.
        if track_id.is_some() {
            let audio_size = fs::metadata(&audio_input)?.len();
            self.space(RESERVE.saturating_add(audio_size.saturating_mul(8)))?;
        }
        let info = self.step(
            Stage::Inspect,
            "inspect",
            tools.info(&audio_input, &config, "dts"),
        )?;
        if !tools::validate_input(&read_json(&info)?) {
            return Err(
                "Input audio is not recognized as 48 kHz DTS:X. Check the source or selected track."
                    .into(),
            );
        }
        let master_base = self.work.join("master");
        let master = self.work.join("master.atmos");
        self.step(
            Stage::Decode,
            "decode",
            tools.decode(&audio_input, &master_base, &config),
        )?;
        let mut master_bytes = 0_u64;
        for name in [
            "master.atmos",
            "master.atmos.audio",
            "master.atmos.metadata",
        ] {
            let size = fs::metadata(self.work.join(name))?.len();
            if size == 0 {
                return Err(format!("Decoder returned an empty master component ({name}).").into());
            }
            master_bytes = master_bytes.saturating_add(size);
        }
        self.space(RESERVE.saturating_add(master_bytes.saturating_mul(2)))?;
        let partial = self.work.join(format!("export.{}", preset.extension()));
        self.step(
            Stage::Encode,
            "encode",
            tools.encode(preset, &master, &partial, &config),
        )?;
        let exported = OpenOptions::new().write(true).open(&partial)?;
        if exported.metadata()?.len() == 0 {
            return Err("Encoder produced an empty file.".into());
        }
        drop(exported);
        let codec = match preset {
            Preset::DtsHdMa => "dts",
            Preset::TrueHd => "truehd",
        };
        let info = self.step(
            Stage::Verify,
            "verify",
            tools.info(&partial, &config, codec),
        )?;
        if !tools::validate_output(&read_json(&info)?, preset) {
            return Err(format!(
                "Encoder output did not pass {codec} profile validation. See {}",
                info.display()
            )
            .into());
        }
        self.step(
            Stage::Verify,
            "verify-decode",
            tools.verify_decode(&partial, &self.work.join("verified"), &config, codec),
        )?;
        let partial = if let (Some(mkv), Some(plan)) = (&mkv, remux) {
            self.remux(mkv, &input, &partial, plan, preset)?
        } else if let Some(mkv) = mkv.as_ref().filter(|_| mka) {
            self.mux_audio(mkv, &partial, preset)?
        } else {
            partial
        };
        OpenOptions::new().write(true).open(&partial)?.sync_all()?;
        self.check_cancel()?;
        let warning = matches!(preset, Preset::TrueHd).then(|| {
            "TrueHD spatial protection is unkeyed. Receiver playback is unverified.".to_string()
        });
        self.attempt.set(self.state, Stage::Publishing, None)?;
        self.check_cancel()?;
        windows::rename(&partial, &output, false).map_err(|error| {
            if output.symlink_metadata().is_ok() {
                format!(
                    "Destination already exists. Choose a new file name: {}",
                    output.display()
                )
            } else {
                format!("Could not publish to {}: {error}", output.display())
            }
        })?;
        Ok(warning)
    }
}

fn require_space(available: u64, minimum: u64) -> Result<()> {
    if available < minimum {
        return Err(format!("Insufficient space on the destination volume. Free at least {} MiB more or choose another destination.",
                           (minimum - available).div_ceil(MIB)).into());
    }
    Ok(())
}

fn tail(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(4096)))?;
    let mut data = Vec::new();
    file.take(4096).read_to_end(&mut data)?;
    Ok(String::from_utf8_lossy(&data).into_owned())
}

fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_reader(File::open(path)?)
        .map_err(|error| format!("Could not read decoder report {}: {error}", path.display()))?)
}

fn read_matroska(path: &Path) -> Result<Identification> {
    let report: Identification = serde_json::from_reader(File::open(path)?)
        .map_err(|error| format!("Could not read MKVToolNix identification: {error}"))?;
    if !report.is_matroska() {
        return Err("MKVToolNix did not identify a supported Matroska file.".into());
    }
    Ok(report)
}

fn compare_payload(raw: &Path, extracted: &Path, cancel: &AtomicBool) -> Result<()> {
    let (raw, extracted) = (File::open(raw)?, File::open(extracted)?);
    let mut remaining = raw.metadata()?.len();
    let different = "Verification failed. The extracted bytes differ from the source.";
    if remaining != extracted.metadata()?.len() {
        return Err(different.into());
    }
    let (mut raw, mut extracted) = (BufReader::new(raw), BufReader::new(extracted));
    let (mut left, mut right) = ([0; 64 * 1024], [0; 64 * 1024]);
    while remaining > 0 {
        check_cancel(cancel)?;
        let count = remaining.min(left.len() as u64) as usize;
        raw.read_exact(&mut left[..count])?;
        extracted.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] {
            return Err(different.into());
        }
        remaining -= count as u64;
    }
    Ok(())
}

pub fn convert(
    state: &Path,
    input: &Path,
    output: &Path,
    preset: Preset,
    track_id: Option<u32>,
    mkv_mode: Option<MkvMode>,
    cancel: &AtomicBool,
) -> Result<Attempt> {
    if has_extension(input, "mkv") && track_id.is_none() {
        return Err("Run tracks to list the MKV audio tracks, then pass --track ID.".into());
    }
    if track_id.is_some() && !has_extension(input, "mkv") {
        return Err("--track ID requires an .mkv input.".into());
    }
    if has_extension(output, "mkv") != mkv_mode.is_some()
        || (mkv_mode.is_some() && track_id.is_none())
    {
        return Err(
            "A new .mkv requires MKV input, a track ID, and explicit add or replace mode.".into(),
        );
    }
    if mkv_mode.is_none()
        && !has_extension(output, preset.extension())
        && !has_extension(output, "mka")
    {
        return Err(format!(
            "Choose a .{} or .mka output file for this preset.",
            preset.extension()
        )
        .into());
    }
    let filename = output.file_name().ok_or("Choose an output file name.")?;
    if filename.to_string_lossy().contains(':') {
        return Err("Choose an ordinary output file name without a colon.".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let output = parent.join(filename);
    let input = input
        .canonicalize()
        .map_err(|error| format!("Cannot open input {}: {error}", input.display()))?;
    fs::create_dir_all(state)?;
    let _lock = lock(state).map_err(|error| {
        format!(
            "Cannot start a job in {}. Another job may be running: {error}",
            state.display()
        )
    })?;
    if let Some(mut previous) = load(state)? {
        recover(state, &mut previous)?;
    }
    let timestamp = now();
    let id = format!("{timestamp}-{}", std::process::id());
    let mut attempt = Attempt {
        id,
        input,
        output,
        preset,
        track_id,
        mkv_mode,
        stage: Stage::Preflight,
        started_unix_ms: timestamp,
        updated_unix_ms: timestamp,
        detail: None,
    };
    let work = attempt.workspace()?;
    let logs = state.join("logs").join(&attempt.id);
    fs::create_dir_all(&logs)?;
    attempt.set(state, Stage::Preflight, None)?;
    let mut job = Job {
        state,
        work: work.clone(),
        logs,
        cancel,
        attempt: &mut attempt,
        owns_work: false,
    };
    let result = Tools::resolve()
        .map_err(|error| format!("Cannot resolve the staged codec tools: {error}").into())
        .and_then(|tools| job.convert(&tools));
    let cleanup = if job.owns_work {
        clean(&work).err()
    } else {
        None
    }
    .map(|error| format!("Could not remove the workspace {}: {error}", work.display()));
    let (stage, detail) = match &result {
        Ok(warning) => (Stage::Succeeded, warning.clone()),
        Err(error) => (
            if cancel.load(Ordering::Relaxed) {
                Stage::Cancelled
            } else {
                Stage::Failed
            },
            Some(error.to_string()),
        ),
    };
    let detail = match (detail, cleanup) {
        (Some(message), Some(cleanup)) => Some(format!("{message} {cleanup}")),
        (first, second) => first.or(second),
    };
    attempt.set(state, stage, detail)?;
    result.map(|_| attempt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_shortage_reports_required_bytes() {
        assert!(require_space(RESERVE, RESERVE).is_ok());
        assert!(
            require_space(RESERVE - 1, RESERVE)
                .unwrap_err()
                .to_string()
                .contains("1 MiB more")
        );
    }
}
