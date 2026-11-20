use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use serde::Serialize;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::{
    desktop,
    mkv::{MkvMode, Track},
    runner::{self, Preset, has_extension},
    windows,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Http = Response<io::Cursor<Vec<u8>>>;
type PickFile = dyn Fn(&str, bool) -> io::Result<Option<PathBuf>> + Send + Sync;
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

struct Session {
    selected: Selection,
    picker: bool,
    cancel: Option<Arc<AtomicBool>>,
    outcome: Option<String>,
}

struct App {
    state_dir: PathBuf,
    host: String,
    origin: String,
    token: String,
    session: Mutex<Session>,
    pick_file: Box<PickFile>,
}

pub fn serve(state_dir: PathBuf, launch: bool) -> Result<()> {
    serve_with_picker(state_dir, launch, Box::new(desktop::pick_file))
}

fn serve_with_picker(state_dir: PathBuf, launch: bool, pick_file: Box<PickFile>) -> Result<()> {
    std::fs::create_dir_all(&state_dir)?;
    let state_dir = state_dir.canonicalize()?;
    let server = Server::http(("127.0.0.1", 0))?;
    let host = server
        .server_addr()
        .to_ip()
        .ok_or("Expected a loopback TCP server")?
        .to_string();
    let origin = format!("http://{host}");
    let token = desktop::token()?;
    let url = format!("{origin}/#{token}");
    let app = Arc::new(App {
        state_dir,
        host,
        origin,
        token,
        pick_file,
        session: Mutex::new(Session {
            selected: Selection {
                input: None,
                output: None,
                preset: Preset::DtsHdMa,
                mka: false,
                mkv_mode: None,
                tracks: Vec::new(),
                track_id: None,
            },
            picker: false,
            cancel: None,
            outcome: None,
        }),
    });
    windows::install_console_handler()?;
    if launch {
        if let Err(error) = desktop::open_browser(&url) {
            eprintln!("{error} Open this URL in your browser:");
            println!("{url}");
        }
    } else {
        println!("{url}");
    }

    let requests = Arc::new(AtomicUsize::new(0));
    while !windows::CANCELLED.load(Ordering::Relaxed) {
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
            let response = app.handle(&request);
            let _ = request.respond(response);
            requests.fetch_sub(1, Ordering::Relaxed);
        });
    }
    loop {
        if let Some(cancel) = &app.session.lock().unwrap().cancel {
            cancel.store(true, Ordering::Relaxed);
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

fn equal_secret(actual: &str, expected: &str) -> bool {
    actual.len() == expected.len()
        && actual
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

impl App {
    fn handle(self: &Arc<Self>, request: &Request) -> Http {
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
            (&Method::Post, "/api/choose-input") => self.choose(false),
            (&Method::Post, "/api/choose-output") => self.choose(true),
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
            (&Method::Post, "/api/convert") => self.start(),
            (&Method::Post, "/api/cancel") => {
                let session = self.session.lock().unwrap();
                if let Some(cancel) = &session.cancel {
                    cancel.store(true, Ordering::Relaxed);
                    json_response(202, json!({}))
                } else {
                    problem(409, "There is no active job in this session.")
                }
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
                json_response(
                    200,
                    json!({
                        "selection": session.selected,
                        "attempt": attempt,
                        "running": session.cancel.is_some() || externally_active,
                        "cancellable": session.cancel.is_some(),
                        "picker": session.picker,
                        "progress": progress,
                        "error": session.outcome,
                    }),
                )
            }
            Err(error) => problem(500, &error.to_string()),
        }
    }

    fn preset(&self, preset: Preset) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || session.cancel.is_some() {
            return problem(409, "Finish the current action before changing the preset.");
        }
        if session.selected.preset != preset {
            session.selected.preset = preset;
            session.selected.output = None;
            session.outcome = None;
        }
        json_response(200, json!({}))
    }

    fn format(&self, mka: bool, mkv_mode: Option<MkvMode>) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || session.cancel.is_some() {
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
            session.selected.output = None;
            session.outcome = None;
        }
        json_response(200, json!({}))
    }

    fn track(&self, id: u32) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || session.cancel.is_some() {
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
        session.outcome = None;
        json_response(200, json!({}))
    }

    fn choose(&self, save: bool) -> Http {
        let extension = {
            let mut session = self.session.lock().unwrap();
            if session.picker || session.cancel.is_some() {
                return problem(409, "Finish the current action before choosing a file.");
            }
            session.picker = true;
            if save && session.selected.mkv_mode.is_some() {
                "mkv"
            } else if save && session.selected.mka {
                "mka"
            } else if save {
                session.selected.preset.extension()
            } else {
                "dts"
            }
        };
        let selected = (|| -> Result<_> {
            let Some(path) = (self.pick_file)(extension, save)? else {
                return Ok(None);
            };
            let path = approve(path, extension, save)?;
            let tracks = if !save && has_extension(&path, "mkv") {
                runner::list_tracks(&self.state_dir, &path, &windows::CANCELLED)?
            } else {
                Vec::new()
            };
            Ok(Some((path, tracks)))
        })();
        let mut session = self.session.lock().unwrap();
        session.picker = false;
        match selected {
            Ok(Some((path, tracks))) => {
                if save {
                    session.selected.output = Some(path);
                } else {
                    if !has_extension(&path, "mkv") && session.selected.mkv_mode.take().is_some() {
                        session.selected.output = None;
                    }
                    session.selected.input = Some(path);
                    session.selected.tracks = tracks;
                    session.selected.track_id = None;
                }
                session.outcome = None;
                json_response(200, json!({"selected": true}))
            }
            Ok(None) => json_response(200, json!({"selected": false})),
            Err(error) => problem(400, &error.to_string()),
        }
    }

    fn start(self: &Arc<Self>) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || session.cancel.is_some() {
            return problem(
                409,
                "Finish the current action before starting another job.",
            );
        }
        let Selection {
            input: Some(input),
            output: Some(output),
            preset,
            track_id,
            mkv_mode,
            ..
        } = session.selected.clone()
        else {
            return problem(
                400,
                "Choose a source and destination in the Windows file pickers.",
            );
        };
        if has_extension(&input, "mkv") && track_id.is_none() {
            return problem(400, "Choose an audio track from the source MKV.");
        }
        let cancel = Arc::new(AtomicBool::new(false));
        session.cancel = Some(Arc::clone(&cancel));
        session.outcome = None;
        let app = Arc::clone(self);
        thread::spawn(move || {
            let result = (|| {
                if input.canonicalize()? != input
                    || output
                        .parent()
                        .ok_or("Choose a destination folder.")?
                        .canonicalize()?
                        != output.parent().unwrap()
                    || output
                        .symlink_metadata()
                        .is_ok_and(|entry| entry.is_symlink())
                {
                    return Err("The selected file or folder changed. Choose it again.".into());
                }
                runner::convert(
                    &app.state_dir,
                    &input,
                    &output,
                    preset,
                    track_id,
                    mkv_mode,
                    &cancel,
                )
            })();
            let mut session = app.session.lock().unwrap();
            session.outcome = result.err().map(|error| error.to_string());
            session.cancel = None;
        });
        json_response(202, json!({}))
    }
}

fn approve(path: PathBuf, extension: &str, save: bool) -> io::Result<PathBuf> {
    if !save {
        let input = path.canonicalize()?;
        if !input.is_file() || !(has_extension(&input, "dts") || has_extension(&input, "mkv")) {
            return Err(io::Error::other("Choose a .dts or .mkv file."));
        }
        return Ok(input);
    }
    if !has_extension(&path, extension) {
        return Err(io::Error::other(format!("Choose a .{extension} file.")));
    }
    let parent = path.parent().unwrap_or(Path::new(".")).canonicalize()?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Choose a file name."))?;
    if name.to_string_lossy().contains(':') {
        return Err(io::Error::other("Choose a file name without a colon."));
    }
    Ok(parent.join(name))
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
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
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
        let source =
            std::env::var_os("XFOLD_BROWSER_TEST_SOURCE").unwrap_or_else(|| "source.dts".into());
        serve_with_picker(
            root.join("browser-state"),
            false,
            Box::new(move |extension, save| {
                Ok(Some(if save {
                    root.join(format!("export.{extension}"))
                } else {
                    root.join(&source)
                }))
            }),
        )
        .unwrap();
    }
}
