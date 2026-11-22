use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::json;

use super::{App, Http, Result, desktop, json_response, problem, queue::JobState, runner};

const REPOSITORY: &str = "VulcanGH/xfold";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn certificate() -> Option<&'static str> {
    option_env!("XFOLD_UPDATE_CERT_SHA256").filter(|pin| {
        pin.len() == 64 && pin.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn installed_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    if !exe.with_file_name(".xfold-payload").is_file()
        || !exe.with_file_name("update.ps1").is_file()
    {
        return Err("Install XFold using Windows Setup to use in-app updates.".into());
    }
    Ok(exe)
}

pub(super) fn enabled() -> bool {
    certificate().is_some() && installed_exe().is_ok()
}

fn powershell(script: &Path, mode: &str, exe: &Path) -> Result<Command> {
    let certificate = certificate().ok_or("This build has no update signing certificate.")?;
    let system = std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable.")?;
    let mut command = Command::new(
        PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    );
    command
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .args(["-Mode", mode, "-Repository", REPOSITORY, "-Current", VERSION, "-Certificate", certificate, "-Application"])
        .arg(exe);
    Ok(command)
}

fn run(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        let message = error.lines().filter(|line| !line.trim().is_empty()).next_back();
        return Err(message.unwrap_or("Windows could not complete the update request.").to_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub(super) fn version(value: &str) -> Option<[u32; 3]> {
    let mut parts = value.split('.');
    let mut parsed = [0; 3];
    for number in &mut parsed {
        let part = parts.next()?;
        *number = part.parse().ok()?;
        if *number > 65_535 || part != number.to_string() {
            return None;
        }
    }
    parts.next().is_none().then_some(parsed)
}

pub(super) fn check() -> Http {
    let result = (|| -> Result<_> {
        let exe = installed_exe()?;
        let available = run(&mut powershell(&exe.with_file_name("update.ps1"), "Check", &exe)?)?;
        if available.is_empty() {
            Ok(None)
        } else if version(&available).is_some() && version(&available) > version(VERSION) {
            Ok(Some(available))
        } else {
            Err("GitHub did not report a valid newer XFold version.".into())
        }
    })();
    match result {
        Ok(available) => json_response(200, json!({ "version": available })),
        Err(error) => problem(502, &error.to_string()),
    }
}

impl App {
    pub(super) fn install_update(&self, target: &str) -> Http {
        if version(target).is_none() || version(target) <= version(VERSION) {
            return problem(400, "Choose a newer XFold version.");
        }
        let Ok(mut restarting) = self.update_gate.try_write() else {
            return problem(409, "Finish the current XFold action before updating.");
        };
        if *restarting {
            return problem(409, "XFold is already restarting.");
        }
        {
            let session = self.session.lock().unwrap();
            if session.active.is_some()
                || session.picker
                || session.jobs.iter().any(|job| job.state == JobState::Waiting)
                || (session.selected.input.is_some() && session.selection_job.is_none())
            {
                return problem(409, "Finish the conversion queue and clear any draft before updating.");
            }
        }
        match runner::status(&self.state_dir) {
            Ok(Some(job)) if job.active() => {
                return problem(409, "Wait for the command-line conversion to finish.");
            }
            Err(error) => return problem(500, &error.to_string()),
            _ => (),
        }
        match self.stage_update(target) {
            Ok(()) => {
                *restarting = true;
                json_response(202, json!({}))
            }
            Err(error) => problem(502, &error.to_string()),
        }
    }

    fn stage_update(&self, target: &str) -> Result<()> {
        let exe = installed_exe()?;
        let stage = self.state_dir.join(format!("update-{}", desktop::token()?));
        fs::create_dir(&stage)?;
        let result = (|| -> Result<()> {
            let script = stage.join("update.ps1");
            fs::copy(exe.with_file_name("update.ps1"), &script)?;
            run(powershell(&script, "Prepare", &exe)?.args(["-Target", target]))?;
            // The worker must survive this process and wait for its executable to be released.
            powershell(&script, "Apply", &exe)?
                .args(["-Target", target, "-WaitFor", &std::process::id().to_string(), "-StateDirectory"])
                .arg(&self.state_dir)
                .spawn()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(stage);
        }
        result
    }
}
