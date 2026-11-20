"use strict";
function element(id) {
    return document.getElementById(id);
}
const controls = element("controls");
const convert = element("convert");
const cancel = element("cancel");
const dts = element("dts");
const truehd = element("truehd");
const notice = element("notice");
const diagnostics = element("diagnostics");
const progress = element("progress");
const theme = element("theme");
const track = element("track");
const format = element("format");
const launcher = location.hash.slice(1);
if (/^[0-9a-f]{64}$/.test(launcher)) {
    sessionStorage.setItem("xfold-session", launcher);
    history.replaceState(null, "", "/");
}
const token = sessionStorage.getItem("xfold-session");
let snapshot = null;
let action = null;
let announced = "";
let lastRequest = 0;
let appliedRequest = 0;
let lastConnectionError = "";
let diagnosticsRevision = 0;
let trackChoices = "";
const stageNames = {
    preflight: "Preparing", inspect: "Inspecting source", extract: "Extracting audio", decode: "Decoding DTS:X",
    encode: "Encoding", mux: "Writing Matroska", verify: "Verifying output", publishing: "Saving export",
    succeeded: "Complete", failed: "Failed", cancelled: "Cancelled", interrupted: "Interrupted",
};
function showNotice(message) {
    if (notice.textContent !== message)
        notice.textContent = message;
    notice.hidden = !message;
}
async function api(path, method = "GET") {
    const response = await fetch(path, {
        method, cache: "no-store",
        headers: { Authorization: `Bearer ${token ?? ""}` },
    });
    const data = await response.json();
    if (!response.ok)
        throw new Error(data.error ?? "XFold could not complete the request.");
    return data;
}
function displayPath(path) {
    const unc = "\\\\?\\UNC\\";
    const extended = "\\\\?\\";
    if (path.startsWith(unc))
        return "\\\\" + path.slice(unc.length);
    return path.startsWith(extended) ? path.slice(extended.length) : path;
}
function render() {
    if (!snapshot)
        return;
    const { selection, attempt, running, cancellable, picker, error } = snapshot;
    const busy = Boolean(action || picker || running);
    const mkv = selection.input?.toLowerCase().endsWith(".mkv") ?? false;
    controls.disabled = busy;
    convert.disabled = busy || !selection.input || !selection.output || (mkv && selection.track_id === null);
    cancel.hidden = !cancellable;
    cancel.disabled = action === "cancel";
    dts.checked = selection.preset === "dts-hd-ma";
    truehd.checked = selection.preset === "true-hd";
    element("input-path").textContent = selection.input ? displayPath(selection.input) : "Choose a .dts or .mkv file";
    element("output-path").textContent = selection.output ? displayPath(selection.output) : "Choose where to save the export";
    element("add-mode").disabled = !mkv;
    element("replace-mode").disabled = !mkv;
    format.value = selection.mkv_mode ?? (selection.mka ? "mka" : "raw");
    const modeHelp = element("mkv-mode-help");
    modeHelp.hidden = !selection.mkv_mode;
    modeHelp.textContent = selection.mkv_mode === "replace"
        ? "The new MKV replaces only the selected track. Your source stays intact."
        : "The new MKV keeps the selected track and its default flag.";
    element("track-field").hidden = !mkv;
    const choices = JSON.stringify(selection.tracks);
    if (choices !== trackChoices) {
        trackChoices = choices;
        const placeholder = new Option(selection.tracks.some(item => item.properties.codec_id === "A_DTS")
            ? "Choose a DTS track" : "No DTS audio tracks found", "");
        placeholder.disabled = true;
        track.replaceChildren(placeholder, ...selection.tracks.map(item => {
            const { language, language_ietf, track_name, codec_id } = item.properties;
            const description = [`Track ${item.id}`, item.codec, language_ietf ?? language, track_name].filter(Boolean).join(" · ");
            const option = new Option(description, String(item.id));
            option.disabled = codec_id !== "A_DTS";
            return option;
        }));
    }
    track.value = selection.track_id === null ? "" : String(selection.track_id);
    const previous = running && (!attempt || ["succeeded", "failed", "cancelled", "interrupted"].includes(attempt.stage));
    const job = previous ? null : attempt;
    const phase = job?.stage ?? "idle";
    const retry = !running && (error || ["failed", "cancelled", "interrupted"].includes(phase))
        && selection.input && selection.output;
    convert.textContent = retry ? "Retry conversion" : "Start conversion";
    const stage = element("stage");
    const label = error && !running && phase !== "cancelled" ? "Failed" :
        previous ? "Preparing" : stageNames[phase] ?? "Ready";
    stage.textContent = label;
    stage.dataset.state = error && !running && phase !== "cancelled" ? "failed" : phase;
    const seconds = job ? Math.max(0, Math.floor(((running ? Date.now() : job.updated_unix_ms) - job.started_unix_ms) / 1000)) : 0;
    element("elapsed").textContent = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
    progress.hidden = !running;
    element("percent").hidden = !running || snapshot.progress === null;
    element("percent").textContent = snapshot.progress === null ? "" : `${snapshot.progress}%`;
    if (snapshot.progress !== null)
        progress.value = snapshot.progress;
    else
        progress.removeAttribute("value");
    const detail = error ?? job?.detail ?? (running ? "Conversion is in progress."
        : phase === "succeeded" ? "Your audio export is ready." : "Choose a source and destination to start.");
    element("detail").textContent = detail;
    element("job-file").textContent = job ? displayPath(job.output) : "";
    diagnostics.hidden = !job;
    const announcement = [job?.id, label, running ? "" : detail].join("|");
    if (announced !== announcement) {
        announced = announcement;
        element("announcement").textContent = running ? label : `${label}. ${detail}`;
    }
}
async function refresh() {
    const revision = ++lastRequest;
    try {
        const next = await api("/api/status");
        if (revision < appliedRequest)
            return;
        appliedRequest = revision;
        snapshot = next;
        if (lastConnectionError)
            showNotice("");
        lastConnectionError = "";
        render();
        if (diagnostics.open && next.attempt)
            void refreshDiagnostics();
    }
    catch (error) {
        if (revision < appliedRequest)
            return;
        lastConnectionError = error instanceof Error ? error.message : String(error);
        showNotice(lastConnectionError);
        controls.disabled = true;
        convert.disabled = true;
        cancel.disabled = true;
    }
}
async function refreshDiagnostics() {
    const revision = ++diagnosticsRevision;
    try {
        const logs = await api("/api/diagnostics");
        if (revision !== diagnosticsRevision)
            return;
        element("log").textContent = logs.map(([name, text]) => `${name}\n${text.trim()}`).join("\n\n")
            || "No codec output is available yet.";
    }
    catch {
        if (revision === diagnosticsRevision)
            element("log").textContent = "Diagnostics are unavailable.";
    }
}
async function post(path, control, waiting = "") {
    if (action)
        return;
    action = path;
    showNotice(waiting);
    render();
    try {
        await api(`/api/${path}`, "POST");
        showNotice("");
    }
    catch (error) {
        showNotice(error instanceof Error ? error.message : String(error));
    }
    finally {
        action = null;
        await refresh();
        if (control.closest("fieldset") !== controls || !controls.disabled)
            control.focus();
        if (path === "convert" && snapshot?.running)
            cancel.focus();
    }
}
function setTheme(mode) {
    document.documentElement.dataset.theme = mode;
    theme.setAttribute("aria-pressed", String(mode === "dark"));
}
setTheme(localStorage.getItem("xfold-theme") ?? "dark");
theme.addEventListener("click", () => {
    const mode = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    localStorage.setItem("xfold-theme", mode);
    setTheme(mode);
});
const input = element("input");
const output = element("output");
input.addEventListener("click", () => void post("choose-input", input, "Use the Windows file picker to choose a source."));
output.addEventListener("click", () => void post("choose-output", output, "Use the Windows file picker to choose a destination."));
for (const radio of [dts, truehd]) {
    radio.addEventListener("change", () => void post(`preset/${radio.value}`, radio));
}
track.addEventListener("change", () => void post(`track/${track.value}`, track));
format.addEventListener("change", () => void post(`format/${format.value}`, format));
convert.addEventListener("click", () => void post("convert", convert));
cancel.addEventListener("click", () => void post("cancel", cancel));
diagnostics.addEventListener("toggle", () => {
    if (diagnostics.open)
        void refreshDiagnostics();
});
if (token) {
    void refresh();
    window.setInterval(() => void refresh(), 800);
}
else {
    showNotice("Open XFold from its launcher to start a session.");
}
