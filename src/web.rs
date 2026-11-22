use std::{
    ffi::OsStr,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::{
    desktop,
    mkv::{MkvMode, Track},
    runner::{self, Preset, has_extension},
    windows,
};
mod preferences;
use preferences::Preferences;
mod queue;
use queue::{ActiveJob, JobState, QueueEntry};
mod update;
mod webhook;
use webhook::Webhook;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Http = Response<io::Cursor<Vec<u8>>>;
type PickFile = dyn Fn(&str, &str, bool, Option<&Path>, Option<&OsStr>) -> io::Result<Option<PathBuf>>
    + Send
    + Sync;
type Notify = dyn Fn(&Webhook, &serde_json::Value) -> bool + Send + Sync;
const POLL: Duration = Duration::from_millis(250);

#[derive(Clone, Serialize)]
struct Selection {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    preset: Preset,
    mka: bool,
    mkv_mode: Option<MkvMode>,
    tracks: Vec<Track>,
    track_id: Option<u32>,
}

impl Selection {
    fn new(preset: Preset) -> Self {
        Self {
            input: None,
            output: None,
            preset,
            mka: false,
            mkv_mode: None,
            tracks: Vec::new(),
            track_id: None,
        }
    }

    fn output_extension(&self) -> &'static str {
        if self.mkv_mode.is_some() {
            "mkv"
        } else if self.mka {
            "mka"
        } else {
            self.preset.extension()
        }
    }

    fn suggested_output(&self, reserved: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        let input = self.input.as_deref()?;
        let parent = input.parent()?;
        let stem: String = input
            .file_stem()?
            .to_string_lossy()
            .chars()
            .take(100)
            .collect();
        let preset = match self.preset {
            Preset::DtsHdMa => "DTS-HD MA",
            Preset::TrueHd => "TrueHD",
        };
        let extension = self.output_extension();
        for copy in 1..=1_000 {
            let name = if copy == 1 {
                format!("{stem} - {preset}.{extension}")
            } else {
                format!("{stem} - {preset} ({copy}).{extension}")
            };
            let output = parent.join(name);
            if reserved(&output) {
                continue;
            }
            match output.symlink_metadata() {
                Ok(_) => (),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(output),
                Err(_) => break,
            }
        }
        None
    }
}

struct Session {
    selected: Selection,
    preferences: Preferences,
    webhook: Option<Webhook>,
    picker: bool,
    selection_job: Option<u64>,
    jobs: Vec<QueueEntry>,
    next_job: u64,
    active: Option<ActiveJob>,
}

struct App {
    state_dir: PathBuf,
    host: String,
    origin: String,
    port: u16,
    requested_port: u16,
    token: String,
    session: Mutex<Session>,
    update_gate: RwLock<bool>,
    shutdown: AtomicBool,
    update_result: Option<String>,
    pick_file: Box<PickFile>,
    notify: Box<Notify>,
}

pub fn serve(state_dir: PathBuf, launch: bool) -> Result<()> {
    serve_with_picker(
        state_dir,
        launch,
        Box::new(desktop::pick_file),
        Box::new(Webhook::notify),
    )
}

fn serve_with_picker(
    state_dir: PathBuf,
    launch: bool,
    pick_file: Box<PickFile>,
    notify: Box<Notify>,
) -> Result<()> {
    std::fs::create_dir_all(&state_dir)?;
    let state_dir = state_dir.canonicalize()?;
    let preferences = Preferences::load(&state_dir).map_err(|error| {
        format!(
            "Could not read {}: {error}",
            state_dir.join("settings.json").display()
        )
    })?;
    let requested_port = preferences.port;
    let server = match Server::http(("127.0.0.1", requested_port)) {
        Err(error) if requested_port != 0 && error.downcast_ref::<io::Error>().is_some_and(|error| {
            matches!(error.kind(), io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied)
        }) => {
            eprintln!("Could not listen on XFold port {requested_port}. Using an automatic port.");
            Server::http(("127.0.0.1", 0))?
        }
        result => result?,
    };
    let address = server
        .server_addr()
        .to_ip()
        .ok_or("Expected a loopback TCP server")?;
    let host = address.to_string();
    let origin = format!("http://{host}");
    let token = desktop::token()?;
    let url = format!("{origin}/#{token}");
    let webhook = Webhook::load(&state_dir).unwrap_or_else(|_| {
        eprintln!("Could not read stored Discord webhook. Save it again in Settings.");
        None
    });
    let update_result = std::fs::read_to_string(state_dir.join("update-result.txt"))
        .ok().map(|message| message.trim_start_matches('\u{feff}').trim().to_owned());
    let app = Arc::new(App {
        state_dir,
        host,
        origin,
        port: address.port(),
        requested_port,
        token,
        pick_file,
        notify,
        update_gate: RwLock::new(false),
        shutdown: AtomicBool::new(false),
        update_result,
        session: Mutex::new(Session {
            selected: Selection::new(preferences.preset),
            preferences,
            webhook,
            picker: false,
            selection_job: None,
            jobs: Vec::new(),
            next_job: 1,
            active: None,
        }),
    });
    windows::install_console_handler()?;
    let tray_app = Arc::clone(&app);
    let tray = if launch {
        match desktop::Tray::start(url.clone(), move || tray_app.quit_from_tray()) {
            Ok(tray) => Some(tray),
            Err(error) => {
                eprintln!("Could not start the XFold tray icon: {error}");
                None
            }
        }
    } else {
        None
    };
    if launch {
        if let Err(error) = desktop::open_browser(&url) {
            eprintln!("{error} Open this URL in your browser:");
            println!("{url}");
        } else if tray.is_some() {
            desktop::detach_standalone_console();
        }
    } else {
        println!("{url}");
    }

    let requests = Arc::new(AtomicUsize::new(0));
    while !windows::CANCELLED.load(Ordering::Relaxed) && !app.shutdown.load(Ordering::Acquire) {
        let Some(request) = server.recv_timeout(POLL)? else {
            continue;
        };
        if requests.fetch_add(1, Ordering::Relaxed) >= 12 {
            requests.fetch_sub(1, Ordering::Relaxed);
            let _ = request.respond(problem(503, "XFold is busy. Try again shortly."));
            continue;
        }
        let app = Arc::clone(&app);
        let requests = Arc::clone(&requests);
        thread::spawn(move || {
            let mut request = request;
            let response = app.handle(&mut request);
            let restart = response.status_code().0 == 202
                && request.url().starts_with("/api/update/install/");
            let _ = request.respond(response);
            if restart {
                app.shutdown.store(true, Ordering::Release);
            }
            requests.fetch_sub(1, Ordering::Relaxed);
        });
    }
    loop {
        if let Some(active) = &app.session.lock().unwrap().active {
            active.cancel.store(true, Ordering::Relaxed);
        } else {
            return Ok(());
        }
        thread::sleep(POLL);
    }
}

fn unique_header<'a>(
    request: &'a Request,
    name: &'static str,
) -> std::result::Result<Option<&'a str>, ()> {
    let mut matches = request
        .headers()
        .iter()
        .filter(|header| header.field.equiv(name));
    let value = matches.next().map(|header| header.value.as_str());
    if matches.next().is_some() {
        Err(())
    } else {
        Ok(value)
    }
}

fn json_body<T: serde::de::DeserializeOwned>(request: &mut Request, limit: usize) -> Option<T> {
    let length = unique_header(request, "Content-Length")
        .ok()??
        .parse::<usize>()
        .ok()?;
    if !(1..=limit).contains(&length)
        || unique_header(request, "Transfer-Encoding") != Ok(None)
        || unique_header(request, "Content-Type") != Ok(Some("application/json"))
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(length);
    request
        .as_reader()
        .take(length as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() == length)
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}

fn equal_secret(actual: &str, expected: &str) -> bool {
    actual.len() == expected.len()
        && actual
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

impl App {
    fn quit_from_tray(&self) -> std::result::Result<(), &'static str> {
        let mut stopping = self.update_gate.try_write()
            .map_err(|_| "Finish the current action before quitting XFold.")?;
        if *stopping {
            return Err("XFold is already closing.");
        }
        let session = self.session.lock().map_err(|_| "XFold could not check the queue.")?;
        if session.active.is_some() || session.picker
            || session.jobs.iter().any(|job| job.state == JobState::Waiting)
        {
            return Err("Finish the conversion queue and close any file picker before quitting XFold.");
        }
        *stopping = true;
        self.shutdown.store(true, Ordering::Release);
        Ok(())
    }

    fn handle(self: &Arc<Self>, request: &mut Request) -> Http {
        if unique_header(request, "Host") != Ok(Some(self.host.as_str())) {
            return problem(403, "Invalid XFold host.");
        }
        let origin = unique_header(request, "Origin");
        if origin != Ok(None) && origin != Ok(Some(self.origin.as_str())) {
            return problem(403, "Invalid request origin.");
        }
        let path = request.url();
        if !path.starts_with("/api/") {
            return static_asset(request.method(), path);
        }
        let authorization = unique_header(request, "Authorization")
            .ok()
            .flatten()
            .and_then(|value| value.strip_prefix("Bearer "));
        if !authorization.is_some_and(|value| equal_secret(value, &self.token)) {
            return problem(401, "Open XFold from its launcher to start a session.");
        }
        if request.method() == &Method::Post && origin != Ok(Some(self.origin.as_str())) {
            return problem(403, "A same-origin request is required.");
        }
        let _mutation = if request.method() == &Method::Post && !path.starts_with("/api/update/install/") {
            match self.update_gate.try_read() {
                Ok(guard) if !*guard => Some(guard),
                _ => return problem(409, "XFold is preparing an update."),
            }
        } else {
            None
        };
        if request.method() == &Method::Post && path == "/api/settings/webhook" {
            return self.set_webhook(request);
        }
        if request.method() == &Method::Post
            && matches!(path, "/api/choose-input" | "/api/choose-output")
        {
            let save = path == "/api/choose-output";
            return self.choose(request, save);
        }
        if unique_header(request, "Transfer-Encoding") != Ok(None)
            || !matches!(
                unique_header(request, "Content-Length"),
                Ok(None | Some("0"))
            )
        {
            return problem(400, "This endpoint accepts no request body.");
        }
        match (request.method(), path) {
            (&Method::Get, "/api/status") => self.status(),
            (&Method::Post, "/api/update/check") => update::check(),
            (&Method::Post, path) if path.starts_with("/api/update/install/") => {
                self.install_update(path.trim_start_matches("/api/update/install/"))
            }
            (&Method::Post, path) if path.starts_with("/api/settings/") => {
                match path["/api/settings/".len()..].split_once('/') {
                    Some((key, value)) => self.settings(key, value),
                    None => problem(400, "Choose a supported setting."),
                }
            }
            (&Method::Get, "/api/diagnostics") => match runner::status(&self.state_dir) {
                Ok(attempt) => json_response(
                    200,
                    json!(
                        attempt
                            .map(|job| job.diagnostics(&self.state_dir))
                            .unwrap_or_default()
                    ),
                ),
                Err(error) => problem(500, &error.to_string()),
            },
            (&Method::Post, "/api/preset/dts-hd-ma") => self.preset(Preset::DtsHdMa),
            (&Method::Post, "/api/preset/true-hd") => self.preset(Preset::TrueHd),
            (&Method::Post, "/api/format/raw") => self.format(false, None),
            (&Method::Post, "/api/format/mka") => self.format(true, None),
            (&Method::Post, "/api/format/add") => self.format(false, Some(MkvMode::Add)),
            (&Method::Post, "/api/format/replace") => self.format(false, Some(MkvMode::Replace)),
            (&Method::Post, path) if path.starts_with("/api/track/") => {
                match path["/api/track/".len()..].parse::<u32>() {
                    Ok(id) => self.track(id),
                    Err(_) => problem(400, "Choose a track from the source MKV."),
                }
            }
            (&Method::Post, "/api/convert") => self.submit(true),
            (&Method::Post, "/api/reset") => self.reset_workspace(),
            (&Method::Post, "/api/queue/add") => self.submit(false),
            (&Method::Post, path) if path.starts_with("/api/cancel/") => {
                self.cancel_job(&path[12..])
            }
            (&Method::Post, path) if path.starts_with("/api/queue/remove/") => {
                self.remove_job(&path[18..])
            }
            _ => problem(404, "Unknown XFold endpoint."),
        }
    }

    fn status(&self) -> Http {
        let session = self.session.lock().unwrap();
        match runner::status(&self.state_dir) {
            Ok(attempt) => {
                let progress = attempt
                    .as_ref()
                    .and_then(|job| job.progress(&self.state_dir));
                let externally_active = attempt.as_ref().is_some_and(runner::Attempt::active);
                let displayed = session.display_job();
                json_response(
                    200,
                    json!({
                        "selection": session.selected,
                        "preferences": session.preferences,
                        "webhook_configured": session.webhook.is_some(),
                        "notification": displayed.and_then(|job| job.notification),
                        "version": env!("CARGO_PKG_VERSION"),
                        "port": self.port,
                        "requested_port": self.requested_port,
                        "updates_enabled": update::enabled(),
                        "update_result": self.update_result,
                        "attempt": attempt,
                        "running": session.active.is_some() || externally_active,
                        "cancellable": session.active.is_some(),
                        "active_job": session.active.as_ref().map(|job| job.id),
                        "selection_job": session.selection_job,
                        "queue": session.jobs,
                        "picker": session.picker,
                        "progress": progress,
                        "error": displayed.and_then(|job| job.error.as_deref()),
                    }),
                )
            }
            Err(error) => problem(500, &error.to_string()),
        }
    }

    fn set_webhook(&self, request: &mut Request) -> Http {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Change {
            url: String,
        }
        let Some(change) = json_body::<Change>(request, 512) else {
            return problem(400, "Could not read webhook setting.");
        };
        let mut session = self.session.lock().unwrap();
        if change.url.trim().is_empty() {
            if Webhook::remove(&self.state_dir).is_err() {
                return problem(500, "Could not remove the Discord webhook.");
            }
            session.webhook = None;
        } else {
            let Some(webhook) = Webhook::parse(&change.url) else {
                return problem(400, "Paste an HTTPS Discord webhook URL from discord.com.");
            };
            if webhook.save(&self.state_dir).is_err() {
                return problem(500, "Could not securely save the Discord webhook.");
            }
            session.webhook = Some(webhook);
        }
        json_response(200, json!({}))
    }

    fn settings(&self, key: &str, value: &str) -> Http {
        let mut session = self.session.lock().unwrap();
        let mut preferences = session.preferences;
        if !preferences.change(key, value) {
            return problem(400, "Choose a supported setting.");
        }
        if let Err(error) = preferences.save(&self.state_dir) {
            return problem(500, &format!("Could not save settings: {error}"));
        }
        let preset_changed = session.preferences.preset != preferences.preset;
        session.preferences = preferences;
        if preset_changed
            && session.selected.input.is_none()
            && session.active.is_none()
            && !session.picker
        {
            session.selected.preset = session.preferences.preset;
            session.selected.output = None;
        }
        json_response(200, json!({}))
    }

    fn preset(&self, preset: Preset) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || !session.selection_editable() {
            return problem(409, "Finish the current action before changing the preset.");
        }
        if session.selected.preset != preset {
            session.selected.preset = preset;
            session.selected.output = session.suggested_output();
            session.selection_job = None;
        }
        json_response(200, json!({}))
    }

    fn format(&self, mka: bool, mkv_mode: Option<MkvMode>) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || !session.selection_editable() {
            return problem(
                409,
                "Finish the current action before changing the output format.",
            );
        }
        if mkv_mode.is_some()
            && !session
                .selected
                .input
                .as_ref()
                .is_some_and(|p| has_extension(p, "mkv"))
        {
            return problem(400, "Choose a source MKV first.");
        }
        if session.selected.mka != mka || session.selected.mkv_mode != mkv_mode {
            session.selected.mka = mka;
            session.selected.mkv_mode = mkv_mode;
            session.selected.output = session.suggested_output();
            session.selection_job = None;
        }
        json_response(200, json!({}))
    }

    fn track(&self, id: u32) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || !session.selection_editable() {
            return problem(409, "Finish the current action before changing the track.");
        }
        if !session
            .selected
            .tracks
            .iter()
            .any(|track| track.id == id && track.has_codec("A_DTS"))
        {
            return problem(400, "Choose a DTS track from the selected MKV.");
        }
        session.selected.track_id = Some(id);
        session.selection_job = None;
        json_response(200, json!({}))
    }

    fn source_tracks(&self, path: &Path) -> Result<Vec<Track>> {
        if has_extension(path, "mkv") {
            runner::list_tracks(&self.state_dir, path, &windows::CANCELLED)
        } else {
            Ok(Vec::new())
        }
    }

    fn choose(&self, request: &mut Request, save: bool) -> Http {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Choice {
            marker: String,
        }
        let Some(choice) = json_body::<Choice>(request, 128)
            .filter(|choice| desktop::valid_marker(&choice.marker))
        else {
            return problem(
                400,
                "Click the file picker button in the XFold browser tab.",
            );
        };
        let (extension, input, suggestion) = {
            let mut session = self.session.lock().unwrap();
            if session.picker || (save && !session.selection_editable()) {
                return problem(409, "Finish the current action before choosing a file.");
            }
            if save && session.selected.input.is_none() {
                return problem(
                    400,
                    "Select a source file before choosing where to save it.",
                );
            }
            session.picker = true;
            if save {
                (
                    session.selected.output_extension(),
                    session.selected.input.clone(),
                    session.suggested_output(),
                )
            } else {
                ("dts", None, None)
            }
        };
        let result = (|| -> Result<_> {
            let path = (self.pick_file)(
                &choice.marker,
                extension,
                save,
                input.as_deref().and_then(Path::parent),
                suggestion.as_deref().and_then(Path::file_name),
            )?;
            let Some(path) = path else { return Ok(None) };
            let path = approve_picker_path(&path, extension, save)?;
            let tracks = if save {
                Vec::new()
            } else {
                self.source_tracks(&path)?
            };
            Ok(Some((path, tracks)))
        })();
        let mut session = self.session.lock().unwrap();
        session.picker = false;
        match result {
            Ok(None) => json_response(200, json!({"selected": false})),
            Ok(Some((path, tracks))) => {
                if save {
                    if session.reserved_output(&path) {
                        return problem(
                            409,
                            "That destination is already used by a queued job. Choose another filename.",
                        );
                    }
                    session.selected.output = Some(path);
                } else {
                    session.select_source(path, tracks);
                }
                session.selection_job = None;
                json_response(200, json!({"selected": true}))
            }
            Err(error) => problem(400, &error.to_string()),
        }
    }
}

fn approve_picker_path(path: &Path, extension: &str, save: bool) -> io::Result<PathBuf> {
    if !save {
        let path = path.canonicalize()?;
        if path.is_file() && (has_extension(&path, "mkv") || has_extension(&path, "dts")) {
            return Ok(path);
        }
        return Err(io::Error::other("Select an MKV or DTS file."));
    }
    if !has_extension(path, extension) {
        return Err(io::Error::other(format!("Choose a .{extension} file.")));
    }
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("Choose a destination folder."))?
        .canonicalize()?;
    let name = path
        .file_name()
        .filter(|name| !name.to_string_lossy().contains(':'))
        .ok_or_else(|| io::Error::other("Choose an ordinary filename."))?;
    let output = folder.join(name);
    match output.symlink_metadata() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(output),
        Ok(_) => Err(io::Error::other(
            "That file already exists. Choose another name.",
        )),
        Err(error) => Err(error),
    }
}

impl Session {
    fn select_source(&mut self, path: PathBuf, tracks: Vec<Track>) {
        let was_mkv = self
            .selected
            .input
            .as_deref()
            .is_some_and(|path| has_extension(path, "mkv"));
        let is_mkv = has_extension(&path, "mkv");
        if is_mkv && !was_mkv {
            self.selected.mka = self.preferences.mkv_mka;
        }
        if !is_mkv {
            self.selected.mkv_mode = None;
        }
        self.selected.input = Some(path);
        self.selected.track_id = tracks
            .iter()
            .find(|track| track.has_codec("A_DTS"))
            .map(|track| track.id);
        self.selected.tracks = tracks;
        self.selected.output = self.suggested_output();
        self.selection_job = None;
    }
}

fn static_asset(method: &Method, path: &str) -> Http {
    let (kind, body): (&str, &[u8]) = match (method, path) {
        (&Method::Get, "/") => (
            "text/html; charset=utf-8",
            include_bytes!("../ui/index.html"),
        ),
        (&Method::Get, "/app.js") => (
            "text/javascript; charset=utf-8",
            include_bytes!("../ui/app.js"),
        ),
        (&Method::Get, "/style.css") => {
            ("text/css; charset=utf-8", include_bytes!("../ui/style.css"))
        }
        (&Method::Get, "/favicon.ico") => ("image/x-icon", include_bytes!("../ui/xfold.ico")),
        _ => return problem(404, "Unknown XFold page."),
    };
    respond(200, kind, body.to_vec())
}

fn problem(code: u16, message: &str) -> Http {
    json_response(code, json!({"error": message}))
}

fn json_response(code: u16, value: serde_json::Value) -> Http {
    respond(
        code,
        "application/json; charset=utf-8",
        value.to_string().into_bytes(),
    )
}

fn respond(code: u16, content_type: &str, body: Vec<u8>) -> Http {
    let mut response = Response::from_data(body).with_status_code(code);
    for (name, value) in [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("Referrer-Policy", "no-referrer"),
        ("X-Content-Type-Options", "nosniff"),
        ("X-Frame-Options", "DENY"),
        (
            "Content-Security-Policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
        ),
    ] {
        response.add_header(Header::from_bytes(name, value).expect("valid static HTTP header"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Started by scripts/test_web_ui.py with browser test fixtures."]
    fn browser_fixture() {
        let root = PathBuf::from(std::env::var_os("XFOLD_BROWSER_TEST_ROOT").unwrap());
        let receipt = root.join("mock-notification.json");
        let source =
            std::env::var_os("XFOLD_BROWSER_TEST_SOURCE").unwrap_or_else(|| "source.dts".into());
        let next = std::env::var_os("XFOLD_BROWSER_TEST_NEXT_SOURCE").map(|path| root.join(path));
        serve_with_picker(
            root.join("browser-state"),
            false,
            Box::new(move |_marker, extension, save, _directory, _suggestion| {
                Ok(Some(if save {
                    root.join(format!("export.{extension}"))
                } else if let Ok(name) = std::fs::read_to_string(root.join("picker-source.txt")) {
                    root.join(name.trim())
                } else {
                    next.as_ref()
                        .filter(|path| path.is_file())
                        .cloned()
                        .unwrap_or_else(|| root.join(&source))
                }))
            }),
            Box::new(move |_, payload| std::fs::write(&receipt, payload.to_string()).is_ok()),
        )
        .unwrap();
    }
}
