use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use serde::Serialize;
use serde_json::json;

use super::{App, Http, Session, json_response, problem};
use crate::{
    mkv::MkvMode,
    runner::{self, Preset, has_extension},
    windows,
};

#[derive(Clone, Serialize)]
struct JobSpec {
    input: PathBuf,
    output: PathBuf,
    preset: Preset,
    track_id: Option<u32>,
    mkv_mode: Option<MkvMode>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum JobState {
    Waiting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Serialize)]
pub(super) struct QueueEntry {
    id: u64,
    #[serde(flatten)]
    spec: JobSpec,
    pub(super) state: JobState,
    pub(super) error: Option<String>,
    pub(super) notification: Option<&'static str>,
}

#[derive(Clone)]
pub(super) struct ActiveJob {
    pub(super) id: u64,
    pub(super) cancel: Arc<AtomicBool>,
}

impl Session {
    pub(super) fn selection_editable(&self) -> bool {
        self.active.is_none() || self.selection_job.is_none()
    }

    pub(super) fn reserved_output(&self, path: &Path) -> bool {
        let output = path.to_string_lossy().to_lowercase();
        self.jobs.iter().any(|entry| {
            matches!(entry.state, JobState::Waiting | JobState::Running)
                && entry.spec.output.to_string_lossy().to_lowercase() == output
        })
    }

    pub(super) fn suggested_output(&self) -> Option<PathBuf> {
        self.selected
            .suggested_output(|path| self.reserved_output(path))
    }

    pub(super) fn display_job(&self) -> Option<&QueueEntry> {
        if let Some(active) = &self.active {
            return self.jobs.iter().find(|job| job.id == active.id);
        }
        self.jobs.iter().rfind(|job| job.state != JobState::Waiting)
    }

    fn begin_next(&mut self) -> Option<(ActiveJob, JobSpec)> {
        debug_assert!(self.active.is_none());
        if windows::CANCELLED.load(Ordering::Relaxed) {
            return None;
        }
        let entry = self
            .jobs
            .iter_mut()
            .find(|job| job.state == JobState::Waiting)?;
        let active = ActiveJob {
            id: entry.id,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        entry.state = JobState::Running;
        self.active = Some(active.clone());
        Some((active, entry.spec.clone()))
    }
}

impl App {
    pub(super) fn reset_workspace(&self) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker {
            return problem(
                409,
                "Close the Windows picker before refreshing the workspace.",
            );
        }
        let active = session.active.as_ref().map(|job| job.id);
        session.jobs.retain(|entry| Some(entry.id) == active);
        session.selected = super::Selection::new(session.preferences.preset);
        session.selection_job = None;
        json_response(200, json!({}))
    }

    pub(super) fn submit(self: &Arc<Self>, start_now: bool) -> Http {
        let mut session = self.session.lock().unwrap();
        if session.picker || windows::CANCELLED.load(Ordering::Relaxed) {
            return problem(409, "Finish the current action before submitting a job.");
        }
        if start_now {
            if session.active.is_some() {
                return problem(409, "A conversion is already running. Use Add to queue.");
            }
            match runner::status(&self.state_dir) {
                Ok(Some(job)) if job.active() => {
                    return problem(409, "Wait for the command-line conversion to finish.");
                }
                Err(error) => return problem(500, &error.to_string()),
                _ => (),
            }
            if let Some(next) = session.begin_next() {
                self.launch_queue(next);
                return json_response(
                    202,
                    json!({"job": session.active.as_ref().map(|job| job.id)}),
                );
            }
        }
        if session.selection_job.is_some_and(|id| {
            session.jobs.iter().any(|job| {
                job.id == id && !matches!(job.state, JobState::Failed | JobState::Cancelled)
            })
        }) {
            return problem(
                409,
                "This selection was already submitted. Choose another file or change the export settings.",
            );
        }
        let selection = &session.selected;
        let (Some(input), Some(output)) = (&selection.input, &selection.output) else {
            return problem(
                400,
                "Choose a source and destination before submitting a job.",
            );
        };
        if has_extension(input, "mkv") && selection.track_id.is_none() {
            return problem(400, "Choose an audio track from the source MKV.");
        }
        if session.reserved_output(output) {
            return problem(
                409,
                "That destination is already used by a queued job. Choose another filename.",
            );
        }
        match output.symlink_metadata() {
            Ok(_) => return problem(409, "That file already exists. Choose another filename."),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => {
                return problem(400, &format!("Could not check the destination. {error}"));
            }
        }
        let spec = JobSpec {
            input: input.clone(),
            output: output.clone(),
            preset: selection.preset,
            track_id: selection.track_id,
            mkv_mode: selection.mkv_mode,
        };
        let id = session.next_job;
        session.next_job += 1;
        session.selection_job = Some(id);
        session.jobs.push(QueueEntry {
            id,
            spec,
            state: JobState::Waiting,
            error: None,
            notification: None,
        });
        if start_now && let Some(next) = session.begin_next() {
            self.launch_queue(next);
        }
        json_response(202, json!({"job": id}))
    }

    fn launch_queue(self: &Arc<Self>, next: (ActiveJob, JobSpec)) {
        let app = Arc::clone(self);
        thread::spawn(move || app.run_queue(next));
    }

    pub(super) fn cancel_job(&self, id: &str) -> Http {
        let id = id.parse::<u64>().ok();
        let session = self.session.lock().unwrap();
        if let Some(active) = session
            .active
            .as_ref()
            .filter(|active| Some(active.id) == id)
        {
            active.cancel.store(true, Ordering::Relaxed);
            json_response(202, json!({}))
        } else {
            problem(409, "That job is no longer running.")
        }
    }

    pub(super) fn remove_job(&self, id: &str) -> Http {
        let id = id.parse::<u64>().ok();
        let mut session = self.session.lock().unwrap();
        if session.picker {
            return problem(409, "Close the file picker before removing a job.");
        }
        let Some(index) = session
            .jobs
            .iter()
            .position(|job| Some(job.id) == id && job.state == JobState::Waiting)
        else {
            return problem(409, "Only waiting jobs can be removed from the queue.");
        };
        session.jobs.remove(index);
        if session.selection_job == id {
            session.selection_job = None;
        }
        json_response(200, json!({}))
    }

    fn run_queue(self: Arc<Self>, mut next: (ActiveJob, JobSpec)) {
        loop {
            let (active, spec) = next;
            let result = (|| -> super::Result<runner::Attempt> {
                let parent = spec.output.parent().ok_or("Choose a destination folder.")?;
                if spec.input.canonicalize()? != spec.input
                    || parent.canonicalize()? != parent
                    || spec
                        .output
                        .symlink_metadata()
                        .is_ok_and(|entry| entry.is_symlink())
                {
                    return Err("The selected file or folder changed. Choose it again.".into());
                }
                runner::convert(
                    &self.state_dir,
                    &spec.input,
                    &spec.output,
                    spec.preset,
                    spec.track_id,
                    spec.mkv_mode,
                    &active.cancel,
                )
            })();
            let (webhook, following) = {
                let mut session = self.session.lock().unwrap();
                let webhook = if result.is_ok() {
                    session.webhook.clone()
                } else {
                    None
                };
                let entry = session
                    .jobs
                    .iter_mut()
                    .find(|job| job.id == active.id)
                    .unwrap();
                entry.state = match &result {
                    Ok(_) => JobState::Succeeded,
                    Err(_) if active.cancel.load(Ordering::Relaxed) => JobState::Cancelled,
                    Err(_) => JobState::Failed,
                };
                entry.error = result.as_ref().err().map(|error| error.to_string());
                entry.notification = webhook.as_ref().map(|_| "sending");
                session.active = None;
                (webhook, session.begin_next())
            };
            if let (Some(webhook), Ok(attempt)) = (webhook, result) {
                let app = Arc::clone(&self);
                thread::spawn(move || {
                    let sent = (app.notify)(&webhook, &super::webhook::completion_embed(&attempt));
                    let mut session = app.session.lock().unwrap();
                    if let Some(job) = session.jobs.iter_mut().find(|job| job.id == active.id) {
                        job.notification = Some(if sent { "sent" } else { "failed" });
                    }
                });
            }
            let Some(following) = following else { return };
            next = following;
        }
    }
}
