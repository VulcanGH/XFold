type Preset = "dts-hd-ma" | "true-hd";
type Track = {
    id: number;
    codec: string;
    properties: { codec_id: string | null; language: string | null; language_ietf: string | null; track_name: string | null };
};
type Attempt = {
    id: string;
    stage: string;
    started_unix_ms: number;
    updated_unix_ms: number;
    input: string;
    output: string;
    detail: string | null;
};
type QueueJob = {
    id: number;
    input: string;
    output: string;
    preset: Preset;
    mkv_mode: "add" | "replace" | null;
    state: "waiting" | "running" | "succeeded" | "failed" | "cancelled";
    error: string | null;
};
type Snapshot = {
    version: string;
    updates_enabled: boolean;
    update_result: string | null;
    preferences: { theme: "system" | "light" | "dark"; preset: Preset; mkv_mka: boolean; port: number };
    port: number;
    requested_port: number;
    webhook_configured: boolean;
    notification: "sending" | "sent" | "failed" | null;
    selection: { input: string | null; output: string | null; preset: Preset; mka: boolean; mkv_mode: "add" | "replace" | null; tracks: Track[]; track_id: number | null };
    attempt: Attempt | null;
    running: boolean;
    cancellable: boolean;
    active_job: number | null;
    selection_job: number | null;
    queue: QueueJob[];
    picker: boolean;
    progress: number | null;
    error: string | null;
};

function element<T extends HTMLElement>(id: string): T {
    return document.getElementById(id) as T;
}

const controls = element<HTMLFieldSetElement>("controls");
const sourceControls = element<HTMLFieldSetElement>("source-controls");
const input = element<HTMLButtonElement>("input");
const output = element<HTMLButtonElement>("output");
const convert = element<HTMLButtonElement>("convert");
const enqueue = element<HTMLButtonElement>("enqueue");
const cancel = element<HTMLButtonElement>("cancel");
const dts = element<HTMLInputElement>("dts");
const truehd = element<HTMLInputElement>("truehd");
const notice = element("notice");
const diagnostics = element<HTMLDetailsElement>("diagnostics");
const progress = element<HTMLProgressElement>("progress");
const theme = element<HTMLButtonElement>("theme");
const track = element<HTMLSelectElement>("track");
const format = element<HTMLSelectElement>("format");
const preferences = element<HTMLFieldSetElement>("preferences");
const themePreference = element<HTMLSelectElement>("pref-theme");
const presetPreference = element<HTMLSelectElement>("pref-preset");
const formatPreference = element<HTMLSelectElement>("pref-format");
const portPreference = element<HTMLInputElement>("pref-port");
const savePort = element<HTMLButtonElement>("save-port");
const portStatus = element("port-status");
let portDraft: string | null = null;
const webhookInput = element<HTMLInputElement>("webhook");
const saveWebhook = element<HTMLButtonElement>("save-webhook");
const removeWebhook = element<HTMLButtonElement>("remove-webhook");
const checkUpdates = element<HTMLButtonElement>("check-updates");
const installUpdate = element<HTMLButtonElement>("install-update");
const updateStatus = element("update-status");
let availableUpdate: string | null = null;
let updateMessage = "";
const systemTheme = window.matchMedia("(prefers-color-scheme: dark)");
const launcher = location.hash.slice(1);
if (/^[0-9a-f]{64}$/.test(launcher)) {
    sessionStorage.setItem("xfold-session", launcher);
    history.replaceState(null, "", "/");
}
const token = sessionStorage.getItem("xfold-session");
let snapshot: Snapshot | null = null;
let action: string | null = null;
let announced = "";
let lastRequest = 0;
let appliedRequest = 0;
let lastConnectionError = "";
let diagnosticsRevision = 0;
let trackChoices = "";
let renderedQueue = "";
const stageNames: Record<string, string> = {
    preflight: "Preparing", inspect: "Inspecting source", extract: "Extracting audio", decode: "Decoding DTS:X",
    encode: "Encoding", mux: "Writing Matroska", verify: "Verifying output", publishing: "Saving export",
    succeeded: "Complete", failed: "Failed", cancelled: "Cancelled", interrupted: "Interrupted",
};

function showNotice(message: string): void {
    if (notice.textContent !== message) notice.textContent = message;
    notice.hidden = !message;
}

async function api<T>(path: string, method = "GET", payload?: object): Promise<T> {
    const headers: Record<string, string> = { Authorization: `Bearer ${token ?? ""}` };
    if (payload !== undefined) headers["Content-Type"] = "application/json";
    const response = await fetch(path, {
        method, cache: "no-store", headers, body: payload === undefined ? undefined : JSON.stringify(payload),
    });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error ?? "XFold could not complete the request.");
    return data as T;
}

function displayPath(path: string): string {
    const unc = "\\\\?\\UNC\\";
    const extended = "\\\\?\\";
    if (path.startsWith(unc)) return "\\\\" + path.slice(unc.length);
    return path.startsWith(extended) ? path.slice(extended.length) : path;
}

function filename(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
}

function jobNumber(id: number): string {
    return String(id).padStart(2, "0");
}

function renderQueue(jobs: QueueJob[], disabled: boolean): void {
    const signature = JSON.stringify([jobs, disabled]);
    if (signature === renderedQueue) return;
    renderedQueue = signature;
    element("queue-empty").hidden = jobs.length !== 0;
    const list = element("queue-items");
    const focused = list.contains(document.activeElement)
        ? (document.activeElement as HTMLElement).dataset.removeJob : undefined;
    const rows = document.createDocumentFragment();
    const labels = { waiting: "Waiting", running: "Converting", succeeded: "Complete", failed: "Failed", cancelled: "Cancelled" };
    for (const job of jobs) {
        const row = document.createElement("li");
        row.dataset.state = job.state;
        row.className = "queue-row";
        const number = document.createElement("span");
        number.className = "queue-number";
        number.textContent = jobNumber(job.id);
        const main = document.createElement("div");
        main.className = "queue-file";
        const name = document.createElement("strong");
        name.textContent = filename(job.input);
        const output = document.createElement("small");
        const preset = job.preset === "true-hd" ? "TrueHD" : "DTS-HD MA";
        const format = job.mkv_mode === "add" ? "Add track to MKV" : job.mkv_mode === "replace" ? "Replace track in MKV"
            : filename(job.output).split(".").pop()?.toUpperCase();
        output.textContent = `${preset} · ${format}`;
        output.title = displayPath(job.output);
        main.append(name, output);
        if (job.state === "failed" && job.error) {
            const error = document.createElement("small");
            error.className = "queue-error";
            error.textContent = job.error.split("\n", 1)[0];
            main.append(error);
        }
        const actions = document.createElement("div");
        actions.className = "queue-actions";
        const state = document.createElement("span");
        state.className = "queue-state";
        state.textContent = labels[job.state];
        actions.append(state);
        if (job.state === "waiting") {
            const remove = document.createElement("button");
            remove.type = "button";
            remove.dataset.removeJob = String(job.id);
            remove.textContent = "Remove";
            remove.setAttribute("aria-label", `Remove job ${jobNumber(job.id)}, ${name.textContent}`);
            remove.disabled = disabled;
            remove.addEventListener("click", () => void post(`queue/remove/${job.id}`, remove));
            actions.append(remove);
        }
        row.append(number, main, actions);
        rows.append(row);
    }
    list.replaceChildren(rows);
    if (focused) list.querySelector<HTMLButtonElement>(`[data-remove-job="${focused}"]`)?.focus();
}

const languageNames = new Intl.DisplayNames(["en"], { type: "language", fallback: "none" });

function languageLabel(track: Track): string | null {
    const tag = [track.properties.language_ietf, track.properties.language]
        .find(value => value && value.toLowerCase() !== "und");
    if (!tag) return null;
    try {
        const name = languageNames.of(tag);
        return name ? `${name} (${tag})` : tag;
    } catch {
        return tag;
    }
}

function renderSummary(selection: Snapshot["selection"]): void {
    const summary = element("export-summary");
    summary.hidden = !selection.input;
    if (!selection.input) return;
    const selected = selection.tracks.find(item => item.id === selection.track_id);
    const fromMkv = selection.input.toLowerCase().endsWith(".mkv");
    const truehd = selection.preset === "true-hd";
    const preset = truehd ? "TrueHD" : "DTS-HD Master Audio";
    const mode = selection.mkv_mode === "replace" ? "Replace track in new MKV"
        : selection.mkv_mode === "add" ? "Add track to new MKV"
        : selection.mka ? "Matroska audio (.mka)" : `Raw audio (${truehd ? ".thd" : ".dts"})`;
    element("summary-output").textContent = `${preset} · ${mode}`;
    const language = selected ? languageLabel(selected) : null;
    element("summary-language").textContent = !selection.mka && !selection.mkv_mode
        ? "This export has no language tag."
        : fromMkv && !selected ? "Choose the source track to see its language."
        : language ? `Track ${selected!.id} · ${language} will be preserved.`
        : fromMkv ? "The selected track has no language tag." : "The raw source has no language tag.";
}

function showPage(focus = false): void {
    const settings = location.hash === "#settings";
    element("convert-page").hidden = settings;
    element("settings-page").hidden = !settings;
    const activeLink = element(settings ? "settings-link" : "convert-link");
    const inactiveLink = element(settings ? "convert-link" : "settings-link");
    activeLink.setAttribute("aria-current", "page");
    inactiveLink.removeAttribute("aria-current");
    document.title = settings ? "Settings · XFold" : "XFold";
    if (focus) element(settings ? "settings-title-page" : "convert-title").focus();
}

function render(): void {
    if (!snapshot) return;
    const { selection, attempt, running, cancellable, picker, queue, selection_job, active_job } = snapshot;
    const busy = Boolean(action || picker);
    const active = queue.find(job => job.id === active_job);
    const submitted = queue.find(job => job.id === selection_job);
    const waiting = queue.some(job => job.state === "waiting");
    const latest = queue.reduce<QueueJob | undefined>((last, item) => item.state === "waiting" ? last : item, undefined);
    const displayed = active ?? (!selection.input || submitted?.id === latest?.id ? latest : undefined);
    const retry = !running && (submitted?.state === "failed" || submitted?.state === "cancelled");
    const prefs = snapshot.preferences;
    const version = element("version");
    version.textContent = `v${snapshot.version}`;
    version.hidden = false;
    element("about-version").textContent = `XFold ${snapshot.version} · Windows x64`;
    checkUpdates.disabled = !snapshot.updates_enabled || busy;
    installUpdate.hidden = !availableUpdate;
    installUpdate.disabled = busy || running || waiting;
    const updateText = updateMessage || snapshot.update_result ||
        (snapshot.updates_enabled ? "" : "In-app updates require an installed build with a configured signing certificate.");
    if (updateStatus.textContent !== updateText) updateStatus.textContent = updateText;
    preferences.disabled = Boolean(action);
    theme.disabled = Boolean(action);
    element<HTMLButtonElement>("copy-support").disabled = false;
    themePreference.value = prefs.theme;
    presetPreference.value = prefs.preset;
    formatPreference.value = prefs.mkv_mka ? "mka" : "raw";
    const configuredPort = prefs.port ? String(prefs.port) : "";
    const portValue = portDraft ?? configuredPort;
    if (portPreference.value !== portValue) portPreference.value = portValue;
    savePort.disabled = portDraft === null || portDraft === configuredPort;
    const activePort = `Listening on port ${snapshot.port}.`;
    let portText = activePort;
    if (prefs.port !== snapshot.requested_port && prefs.port !== snapshot.port) {
        portText = `${activePort} Restart XFold to apply the saved setting.`;
    } else if (prefs.port && prefs.port !== snapshot.port) {
        portText = `Port ${prefs.port} was unavailable at startup. ${activePort}`;
    }
    if (portStatus.textContent !== portText) portStatus.textContent = portText;
    const webhookState = element("webhook-state");
    const configured = snapshot.webhook_configured ? "Webhook saved." : "Notifications off.";
    if (webhookState.textContent !== configured) webhookState.textContent = configured;
    removeWebhook.hidden = !snapshot.webhook_configured;
    setTheme(prefs.theme);
    renderSummary(selection);
    const mkv = selection.input?.toLowerCase().endsWith(".mkv") ?? false;
    controls.disabled = busy || Boolean(active && submitted);
    sourceControls.disabled = controls.disabled;
    input.disabled = busy;
    input.textContent = active || waiting ? "Add another file" : "Select file";
    const ready = !(submitted && !retry) && !!selection.input && !!selection.output
        && (!mkv || selection.track_id !== null);
    convert.hidden = Boolean(active);
    convert.disabled = busy || running || (!waiting && !ready);
    enqueue.disabled = busy || !ready;
    enqueue.classList.toggle("primary", Boolean(active));
    cancel.hidden = !cancellable;
    cancel.disabled = busy;
    dts.checked = selection.preset === "dts-hd-ma";
    truehd.checked = selection.preset === "true-hd";
    element("input-path").textContent = selection.input ? displayPath(selection.input) : "Choose a .dts or .mkv file";
    const number = element("source-number");
    number.hidden = !submitted;
    number.textContent = submitted ? jobNumber(submitted.id) : "";
    element("output-path").textContent = selection.output ? displayPath(selection.output) : "Select a source to suggest a destination";
    output.disabled = !selection.input;
    element<HTMLOptionElement>("add-mode").disabled = !mkv;
    element<HTMLOptionElement>("replace-mode").disabled = !mkv;
    format.value = selection.mkv_mode ?? (selection.mka ? "mka" : "raw");
    const modeHelp = element("mkv-mode-help");
    modeHelp.hidden = !mkv;
    modeHelp.textContent = selection.mkv_mode === "replace"
        ? "The new MKV replaces only the selected track. Your source stays intact."
        : selection.mkv_mode === "add" ? "The converted track becomes the first and default audio track. The new MKV keeps the original audio tracks."
        : selection.mka ? "Keeps the selected track’s language."
        : "Raw audio cannot store the track’s language. Choose MKA to keep it.";
    element("track-field").hidden = !mkv;
    const choices = JSON.stringify(selection.tracks);
    if (choices !== trackChoices) {
        trackChoices = choices;
        const placeholder = new Option(selection.tracks.some(item => item.properties.codec_id === "A_DTS")
            ? "Choose a DTS track" : "No DTS audio tracks found", "");
        placeholder.disabled = true;
        track.replaceChildren(placeholder, ...selection.tracks.map(item => {
            const { track_name, codec_id } = item.properties;
            const description = [`Track ${item.id}`, item.codec, languageLabel(item), track_name].filter(Boolean).join(" · ");
            const option = new Option(description, String(item.id));
            option.disabled = codec_id !== "A_DTS";
            return option;
        }));
    }
    track.value = selection.track_id === null ? "" : String(selection.track_id);
    const previous = running && (!attempt || ["succeeded", "failed", "cancelled", "interrupted"].includes(attempt.stage)
        || Boolean(active && (active.input !== attempt.input || active.output !== attempt.output)));
    const changed = !running && selection.input && attempt &&
        (selection.input !== attempt.input || selection.output !== attempt.output);
    const job = previous || changed || (!running && !selection.input && !displayed) ? null : attempt;
    const phase = job?.stage ?? "idle";
    const error = !running && displayed ? snapshot.error : null;
    convert.textContent = waiting ? "Start queue" : retry ? "Retry conversion" : "Start conversion";
    const stage = element("stage");
    const label = error && !running && phase !== "cancelled" ? "Failed" :
        previous ? "Preparing" : stageNames[phase] ?? "Ready";
    stage.textContent = label;
    stage.dataset.state = error && !running && phase !== "cancelled" ? "failed" : phase;
    const seconds = job ? Math.max(0, Math.floor(
        ((running ? Date.now() : job.updated_unix_ms) - job.started_unix_ms) / 1000
    )) : 0;
    element("elapsed").textContent = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
    progress.hidden = !running;
    element("percent").hidden = !running || snapshot.progress === null;
    const estimated = phase === "decode" && snapshot.progress !== null;
    element("percent").textContent = snapshot.progress === null ? "" : `${snapshot.progress}%`;
    if (estimated) progress.setAttribute("aria-valuetext", `Approximately ${snapshot.progress}%`);
    else progress.removeAttribute("aria-valuetext");
    if (snapshot.progress !== null) progress.value = snapshot.progress;
    else progress.removeAttribute("value");
    const detail = error ?? job?.detail ?? (running ? "Conversion is in progress."
        : phase === "succeeded" ? "Your audio export is ready." : waiting ? "The queue is ready to start." : ready
            ? "Ready to convert." : "Select a source file to start.");
    element("detail").textContent = detail;
    const current = element("current-source");
    const shown = displayed;
    current.hidden = !shown;
    current.textContent = shown ? `${jobNumber(shown.id)} · ${filename(shown.input)}` : "";
    element("job-file").textContent = job ? displayPath(job.output) : "";
    const notification = element("notification-status");
    const notificationText = phase !== "succeeded" ? "" : snapshot.notification === "sending"
        ? "Sending Discord notification…" : snapshot.notification === "sent"
        ? "Discord notification sent." : snapshot.notification === "failed"
        ? "Discord notification failed. The export was saved." : "";
    notification.hidden = !notificationText;
    if (notification.textContent !== notificationText) notification.textContent = notificationText;
    element("job-actions").hidden = !job;
    element<HTMLButtonElement>("copy-output").hidden = phase !== "succeeded";
    diagnostics.hidden = !job;
    renderQueue(queue, busy);
    const announcement = [active?.id, job?.id, label, running ? "" : detail].join("|");
    if (announced !== announcement) {
        announced = announcement;
        element("announcement").textContent = running ? label : `${label}. ${detail}`;
    }
}

async function refresh(): Promise<void> {
    if (action === "update/restarting") return;
    const revision = ++lastRequest;
    try {
        const next = await api<Snapshot>("/api/status");
        if (revision < appliedRequest) return;
        appliedRequest = revision;
        snapshot = next;
        if (lastConnectionError) showNotice("");
        lastConnectionError = "";
        render();
        if (diagnostics.open && next.attempt) void refreshDiagnostics();
    } catch (error) {
        if (revision < appliedRequest) return;
        lastConnectionError = error instanceof Error ? error.message : String(error);
        showNotice(lastConnectionError);
        controls.disabled = true;
        sourceControls.disabled = true;
        input.disabled = true;
        preferences.disabled = true;
        theme.disabled = true;
        convert.disabled = true;
        enqueue.disabled = true;
        cancel.disabled = true;
        checkUpdates.disabled = true;
        installUpdate.disabled = true;
    }
}

async function refreshDiagnostics(): Promise<void> {
    const revision = ++diagnosticsRevision;
    try {
        const logs = await api<Array<[string, string]>>("/api/diagnostics");
        if (revision !== diagnosticsRevision) return;
        element("log").textContent = logs.map(([name, text]) => `${name}\n${text.trim()}`).join("\n\n")
            || "No codec output is available yet.";
    } catch {
        if (revision === diagnosticsRevision) element("log").textContent = "Diagnostics are unavailable.";
    }
}

async function post(path: string, control: HTMLElement, waiting = "", data?: object): Promise<boolean> {
    if (action) return false;
    const starting = path === "convert" && !snapshot?.running;
    action = path;
    showNotice(waiting);
    render();
    try {
        await api(`/api/${path}`, "POST", data);
        showNotice("");
        return true;
    } catch (error) {
        showNotice(error instanceof Error ? error.message : String(error));
        return false;
    } finally {
        action = null;
        await refresh();
        if (!control.matches(":disabled")) control.focus();
        if (starting && snapshot?.running) cancel.focus();
        else if (path === "queue/add") input.focus();
    }
}

function setTheme(mode: string): void {
    const effective = mode === "system" ? (systemTheme.matches ? "dark" : "light") : mode;
    document.documentElement.dataset.theme = effective;
    theme.textContent = effective === "dark" ? "Light mode" : "Dark mode";
}

portPreference.addEventListener("input", () => {
    portDraft = portPreference.value;
    render();
});
savePort.addEventListener("click", async () => {
    if (!portPreference.reportValidity()) return;
    const port = portPreference.value ? Number(portPreference.value) : 0;
    if (await post(`settings/port/${port}`, savePort)) {
        portDraft = null;
        render();
        portPreference.focus();
    }
});

showPage();
window.addEventListener("hashchange", () => showPage(true));
systemTheme.addEventListener("change", () => {
    if (snapshot?.preferences.theme === "system") setTheme("system");
});
theme.addEventListener("click", () => {
    const mode = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    void post(`settings/theme/${mode}`, theme);
});
for (const [key, control] of [
    ["theme", themePreference], ["preset", presetPreference],
    ["mkv-format", formatPreference],
] as const) {
    control.addEventListener("change", () => void post(`settings/${key}/${control.value}`, control));
}
saveWebhook.addEventListener("click", async () => {
    const url = webhookInput.value.trim();
    if (!url) {
        showNotice("Paste a Discord webhook URL.");
        webhookInput.focus();
    } else if (await post("settings/webhook", saveWebhook, "", { url })) {
        webhookInput.value = "";
    }
});
removeWebhook.addEventListener("click", async () => {
    if (await post("settings/webhook", removeWebhook, "", { url: "" })) {
        webhookInput.value = "";
        webhookInput.focus();
    }
});

async function copy(text: string, success: string): Promise<void> {
    try {
        await navigator.clipboard.writeText(text);
        element("feedback").textContent = success;
        showNotice("");
    } catch {
        showNotice("Could not copy to the clipboard.");
    }
}

element("copy-support").addEventListener("click", () => {
    if (snapshot) void copy(`XFold ${snapshot.version}\nWindows x64`, "Support info copied.");
});
element("copy-output").addEventListener("click", () => {
    if (snapshot?.attempt?.stage === "succeeded") {
        void copy(displayPath(snapshot.attempt.output), "Output path copied.");
    }
});
element("copy-diagnostics").addEventListener("click", async () => {
    try {
        const logs = await api<Array<[string, string]>>("/api/diagnostics");
        const detail = snapshot?.attempt?.detail ?? "";
        await copy([`XFold ${snapshot?.version ?? ""}`, detail,
            ...logs.map(([name, text]) => `${name}\n${text.trim()}`)].filter(Boolean).join("\n\n"),
            "Diagnostics copied.");
    } catch {
        showNotice("Could not load diagnostics.");
    }
});
async function pickFile(route: string, control: HTMLButtonElement): Promise<void> {
    if (action) return;
    const marker = Array.from(crypto.getRandomValues(new Uint8Array(16)), byte =>
        byte.toString(16).padStart(2, "0")).join("");
    const previousTitle = document.title;
    const pickerTitle = `XFold ${marker}`;
    document.title = pickerTitle;
    const restoreTitle = () => {
        if (document.title === pickerTitle) document.title = previousTitle;
    };
    const timer = window.setTimeout(restoreTitle, 2200);
    try {
        await post(route, control, "Use the Windows picker to select a file.", { marker });
    } finally {
        window.clearTimeout(timer);
        restoreTitle();
    }
}

input.addEventListener("click", () => void pickFile("choose-input", input));
output.addEventListener("click", () => void pickFile("choose-output", output));
for (const radio of [dts, truehd]) {
    radio.addEventListener("change", () => void post(`preset/${radio.value}`, radio));
}
track.addEventListener("change", () => void post(`track/${track.value}`, track));
format.addEventListener("change", () => void post(`format/${format.value}`, format));
convert.addEventListener("click", () => void post("convert", convert));
enqueue.addEventListener("click", () => void post("queue/add", enqueue));
cancel.addEventListener("click", () => {
    if (snapshot?.active_job) void post(`cancel/${snapshot.active_job}`, cancel);
});
diagnostics.addEventListener("toggle", () => {
    if (diagnostics.open) void refreshDiagnostics();
});
checkUpdates.addEventListener("click", () => void changeUpdate(false));
installUpdate.addEventListener("click", () => void changeUpdate(true));

async function changeUpdate(install: boolean): Promise<void> {
    if (action || (install && !availableUpdate)) return;
    action = "update";
    updateMessage = install ? "Downloading and verifying the installer…" : "Checking for updates…";
    render();
    try {
        if (install) {
            await api(`/api/update/install/${availableUpdate}`, "POST");
            action = "update/restarting";
            updateMessage = "Installing the update. XFold will open a new browser tab when it restarts.";
        } else {
            const reply = await api<{ version: string | null }>("/api/update/check", "POST");
            availableUpdate = reply.version;
            updateMessage = availableUpdate ? `XFold ${availableUpdate} is available.` : "You're up to date.";
        }
    } catch (error) {
        if (!install) availableUpdate = null;
        updateMessage = error instanceof Error ? error.message : String(error);
    } finally {
        if (action !== "update/restarting") action = null;
        render();
    }
}
async function connect(): Promise<void> {
    const navigation = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined;
    if (navigation?.type === "reload") {
        try {
            await api("/api/reset", "POST");
        } catch (error) {
            showNotice(error instanceof Error ? error.message : String(error));
        }
    }
    await refresh();
    window.setInterval(() => void refresh(), 800);
}

if (token) {
    void connect();
} else {
    showNotice("Open XFold from its launcher to start a session.");
}
