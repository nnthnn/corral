//! OpenCode browser host for corral.
//!
//! OpenCode v2's `browser.*` tools only exist while a desktop client is
//! attached: the server's built-in browser plugin registers an
//! `experimental.browser` RPC, and the desktop long-polls `attach`, then
//! executes commands received over the server-wide SSE event stream.
//!
//! The actual browser is corral's embedded WebKit window (`browser_window.rs`),
//! so no external browser is required. `browser.preview` keeps opening files in
//! the existing File tab. Operations that need Chromium-only devtools
//! (tracing, CPU/heap profiles, Lighthouse) return a clear `unsupported`
//! failure.
//!
//! The host is attached per corral session (each opencode session already has
//! its own dedicated `opencode serve` process), started by
//! `commands::session::start_session` alongside `opencode_events::spawn`.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use corral_core::opencode::{self, BrowserRpcError};

use crate::browser_window::{self, BrowserError, BrowserWindow};

/// Per-session browser hosts, keyed by corral's `<project>/<feature>` id. The
/// local browser chrome page uses this to route its UI commands; the opencode
/// command worker uses the `Arc<Host>` it already owns.
#[derive(Default)]
pub struct BrowserHosts(Mutex<HashMap<String, Arc<Host>>>);

impl BrowserHosts {
    fn get(&self, id: &str) -> Option<Arc<Host>> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn insert(&self, id: String, host: Arc<Host>) -> Option<Arc<Host>> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, host)
    }

    fn remove(&self, id: &str) -> Option<Arc<Host>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(id)
    }
}

struct Host {
    app: AppHandle,
    /// Corral's registry key (`<project>/<feature>`).
    id: String,
    project: String,
    feature: String,
    worktree: PathBuf,
    base_url: String,
    password: String,
    /// opencode's own root session id (`ses_...`).
    session_id: String,
    connection_id: String,
    window: Arc<BrowserWindow>,
}

/// Payload for the frontend `browser-preview` event.
#[derive(Clone, Serialize)]
struct PreviewEvent {
    project: String,
    feature: String,
    path: String,
}

/// Start the browser host for one opencode v2 session. Best-effort: failures
/// are logged, never surfaced as a session-start error — a missing host only
/// means the `browser.*` tools stay unavailable.
pub fn spawn(
    app: AppHandle,
    id: String,
    project: String,
    feature: String,
    worktree: PathBuf,
    session: OpenCodeSession,
) {
    let window = BrowserWindow::new(app.clone(), id.clone());
    let host = Arc::new(Host {
        app: app.clone(),
        id: id.clone(),
        project,
        feature,
        worktree,
        base_url: session.base_url,
        password: session.password,
        session_id: session.root_session_id,
        connection_id: format!("corral-{}", Uuid::new_v4()),
        window,
    });

    if let Some(hosts) = app.try_state::<BrowserHosts>()
        && let Some(previous) = hosts.insert(id.clone(), host.clone())
    {
        // A restarted session for the same worktree: close the previous
        // host's browser window rather than leaving an orphan behind.
        previous.window.close_all();
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let (command_tx, command_rx) = std::sync::mpsc::channel::<String>();
    let events = host.clone();
    std::thread::spawn(move || event_loop(events, ready_tx, command_tx));
    let worker = host.clone();
    std::thread::spawn(move || command_worker(worker, command_rx));
    std::thread::spawn(move || attach_loop(host, ready_rx));
}

/// The opencode server coordinates needed to attach this host.
pub struct OpenCodeSession {
    pub root_session_id: String,
    pub base_url: String,
    pub password: String,
}

impl Host {
    fn shutdown(&self) {
        self.window.close_all();
        if let Some(hosts) = self.app.try_state::<BrowserHosts>() {
            hosts.remove(&self.id);
        }
    }

    fn send_state(&self) {
        let state = self.window.state_json();
        if let Err(e) = opencode::browser_rpc(
            &self.base_url,
            &self.password,
            "state",
            &self.worktree,
            json!({
                "sessionID": self.session_id,
                "connectionID": self.connection_id,
                "state": state,
            }),
        ) {
            tracing::debug!("browser_host: failed to publish state for {}: {e}", self.id);
        }
    }
}

fn attach_loop(host: Arc<Host>, ready: Receiver<()>) {
    // The browser plugin is an internal plugin the server activates lazily
    // after the location boots — a few seconds on a cold start. `attach`
    // rejects with `rpc.unavailable` until then, so retry it rather than
    // giving up and leaving the tools permanently unavailable.
    const MAX_ATTEMPTS: usize = 120;
    let _ = ready.recv_timeout(Duration::from_secs(5));
    for attempt in 0..MAX_ATTEMPTS {
        match opencode::browser_attach(
            &host.base_url,
            &host.password,
            &host.session_id,
            &host.connection_id,
            &host.worktree,
        ) {
            Ok(outcome) => {
                tracing::info!("browser_host: attachment for {} ended ({outcome})", host.id);
                host.shutdown();
                return;
            }
            Err(BrowserRpcError::Unavailable { message }) => {
                tracing::debug!(
                    "browser_host: waiting for browser plugin for {} (attempt {}): {message}",
                    host.id,
                    attempt + 1
                );
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(e) => {
                // A transport error here normally means the paired
                // `opencode serve` process is gone (session ended or the TUI
                // was closed), which is expected.
                tracing::debug!("browser_host: attach for {} failed: {e}", host.id);
                host.shutdown();
                return;
            }
        }
    }
    tracing::warn!(
        "browser_host: browser plugin never became available for {}; \
         browser.* tools stay disabled for this session",
        host.id
    );
    host.shutdown();
}

fn event_loop(host: Arc<Host>, ready: Sender<()>, commands: Sender<String>) {
    let url = format!("{}/api/event", host.base_url);
    // Retry only the *initial* subscription. Once a stream has been
    // established, losing it means the paired server is gone.
    for attempt in 0..30 {
        let request = ureq::get(&url)
            .header("Accept", "text/event-stream")
            .header("Authorization", &opencode::basic_auth(&host.password));
        match request.call() {
            Ok(response) => {
                tracing::info!("browser_host: subscribed for {}", host.id);
                let _ = ready.send(());
                let reader = std::io::BufReader::new(response.into_body().into_reader());
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let Some(data) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    let Ok(event) = serde_json::from_str::<Value>(data) else {
                        continue;
                    };
                    handle_event(&host, &event, &commands);
                }
                tracing::debug!("browser_host: event stream ended for {}", host.id);
                host.shutdown();
                return;
            }
            Err(e) => {
                tracing::debug!(
                    "browser_host: subscribe attempt {attempt} for {} failed: {e}",
                    host.id
                );
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
    tracing::warn!(
        "browser_host: could not subscribe for {}; browser.* tools stay disabled",
        host.id
    );
    host.shutdown();
}

fn command_worker(host: Arc<Host>, commands: Receiver<String>) {
    for request_id in commands {
        handle_command(&host, &request_id);
    }
}

fn handle_event(host: &Host, event: &Value, commands: &Sender<String>) {
    if event.get("type").and_then(Value::as_str) != Some("rpc.experimental.browser.control") {
        return;
    }
    let data = event.get("data").unwrap_or(&Value::Null);
    if data.get("connectionID").and_then(Value::as_str) != Some(host.connection_id.as_str()) {
        return;
    }
    match data.get("type").and_then(Value::as_str) {
        Some("attached") => tracing::info!(
            "browser_host: attached to {} ({})",
            host.id,
            host.session_id
        ),
        Some("command") => {
            if let Some(request_id) = data.get("requestID").and_then(Value::as_str) {
                // The worker executes commands in arrival order; this reader
                // keeps draining the volatile event stream meanwhile.
                let _ = commands.send(request_id.to_string());
            }
        }
        Some("cancel") => {}
        _ => {}
    }
}

fn handle_command(host: &Host, request_id: &str) {
    let command = match opencode::browser_rpc(
        &host.base_url,
        &host.password,
        "command",
        &host.worktree,
        json!({
            "sessionID": host.session_id,
            "connectionID": host.connection_id,
            "requestID": request_id,
        }),
    ) {
        Ok(command) => command,
        Err(e) => {
            tracing::debug!(
                "browser_host: command {request_id} for {} unavailable: {e}",
                host.id
            );
            return;
        }
    };

    let action = command.get("action").cloned().unwrap_or(Value::Null);
    let files = command
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let kind = action
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    tracing::debug!("browser_host: {} -> browser.{kind}", host.id);
    match execute(host, &action, &files) {
        Ok(executed) => {
            // Publishing before the result keeps the server's tab inventory
            // current (and is required for tabs.open/close). It is cheap
            // enough to do for every successful command, including commands
            // that only observed the page.
            host.send_state();
            send_result(
                host,
                request_id,
                json!({
                    "type": "success",
                    "result": { "value": executed.value, "files": executed.files },
                }),
            );
        }
        Err((code, message)) => {
            tracing::warn!(
                "browser_host: {} browser.{kind} failed: [{code}] {message}",
                host.id
            );
            send_result(
                host,
                request_id,
                json!({ "type": "failure", "code": code, "message": message }),
            );
        }
    }
}

struct Executed {
    value: Value,
    files: Vec<Value>,
}

impl Executed {
    fn value(value: Value) -> Self {
        Self {
            value,
            files: Vec::new(),
        }
    }
}

fn execute(host: &Host, action: &Value, files: &[Value]) -> Result<Executed, BrowserError> {
    let kind = action.get("type").and_then(Value::as_str).unwrap_or("");

    match kind {
        "preview" => {
            let raw = action.get("path").and_then(Value::as_str).unwrap_or("");
            preview(host, raw)?;
            Ok(Executed::value(json!({ "path": raw })))
        }
        "tabs.list" => Ok(Executed::value(host.window.state_json())),
        "tabs.open" => {
            let raw = action
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("about:blank");
            let focus = action.get("focus").and_then(Value::as_bool).unwrap_or(true);
            Ok(Executed::value(host.window.open_tab(raw, focus)?))
        }
        "tabs.focus" => {
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.focus_tab(&tab_id)?))
        }
        "tabs.close" => {
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.close_tab(&tab_id)?))
        }
        "navigate" => {
            frame_supported(action)?;
            let tab_id = tab_id(action)?;
            let url = action.get("url").and_then(Value::as_str).unwrap_or("");
            Ok(Executed::value(host.window.navigate(&tab_id, url)?))
        }
        "back" => {
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.go_back(&tab_id)?))
        }
        "forward" => {
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.go_forward(&tab_id)?))
        }
        "reload" => {
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.reload(&tab_id)?))
        }
        "stop" => {
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.stop(&tab_id)?))
        }
        "wait" => {
            frame_supported(action)?;
            let tab_id = tab_id(action)?;
            Ok(Executed::value(host.window.wait(&tab_id, action)?))
        }
        "snapshot" => {
            frame_supported(action)?;
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(
                &tab_id,
                "snapshot",
                json!({
                    "ref": action.get("ref"),
                    "depth": action.get("depth"),
                    "boxes": action.get("boxes"),
                }),
            )?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &["content", "truncated"],
            )?))
        }
        "find" => {
            frame_supported(action)?;
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(
                &tab_id,
                "find",
                json!({
                    "text": action.get("text"),
                    "depth": action.get("depth"),
                    "boxes": action.get("boxes"),
                }),
            )?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &["content", "truncated"],
            )?))
        }
        "evaluate" => {
            frame_supported(action)?;
            let tab_id = tab_id(action)?;
            let script = action.get("script").and_then(Value::as_str).unwrap_or("");
            let reference = action.get("ref").and_then(Value::as_str);
            let value = host.window.evaluate(&tab_id, script, reference)?;
            let tab = host.window.tab_json(&tab_id).ok_or_else(tab_unavailable)?;
            Ok(Executed::value(json!({ "tab": tab, "value": value })))
        }
        "click" | "hover" | "fill" | "fill_form" | "select" | "check" | "press" | "scroll" => {
            let tab_id = tab_id(action)?;
            host.window.agent_call(&tab_id, kind, action.clone())?;
            Ok(Executed::value(
                host.window.tab_json(&tab_id).ok_or_else(tab_unavailable)?,
            ))
        }
        "screenshot" => {
            let tab_id = tab_id(action)?;
            let (value, files) = host.window.screenshot(&tab_id, action)?;
            Ok(Executed { value, files })
        }
        "console" => {
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(
                &tab_id,
                "consoleRead",
                json!({
                    "level": action.get("level"),
                    "limit": action.get("limit"),
                }),
            )?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &["messages", "truncated", "dropped"],
            )?))
        }
        "network.list" => {
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(
                &tab_id,
                "networkList",
                json!({
                    "urlContains": action.get("urlContains"),
                    "resourceType": action.get("resourceType"),
                    "limit": action.get("limit"),
                }),
            )?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &["requests", "truncated", "dropped"],
            )?))
        }
        "network.get" => {
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(
                &tab_id,
                "networkGet",
                json!({
                    "id": action.get("id"),
                    "includeBody": action.get("includeBody"),
                    "maxBodyChars": action.get("maxBodyChars"),
                }),
            )?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &[
                    "request",
                    "requestHeaders",
                    "responseHeaders",
                    "headersTruncated",
                    "requestBody",
                    "responseBody",
                ],
            )?))
        }
        "dialog" => {
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(
                &tab_id,
                "dialogAction",
                json!({
                    "action": action.get("action"),
                    "promptText": action.get("promptText"),
                }),
            )?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &["dialog"],
            )?))
        }
        "frames" => {
            let tab_id = tab_id(action)?;
            let value = host.window.agent_call(&tab_id, "frames", json!({}))?;
            Ok(Executed::value(with_tab(
                host,
                &tab_id,
                value,
                &["frames"],
            )?))
        }
        "files.upload" | "files.drop" => {
            let tab_id = tab_id(action)?;
            let reference = action.get("ref").and_then(Value::as_str).unwrap_or("");
            let uploads: Vec<Value> = files
                .iter()
                .map(|file| {
                    json!({
                        "name": file.get("name"),
                        "mime": file.get("mime"),
                        "data": file.get("data"),
                    })
                })
                .collect();
            host.window.agent_call(
                &tab_id,
                if kind == "files.upload" {
                    "upload"
                } else {
                    "drop"
                },
                json!({ "ref": reference, "files": uploads }),
            )?;
            Ok(Executed::value(
                host.window.tab_json(&tab_id).ok_or_else(tab_unavailable)?,
            ))
        }
        "files.list" | "files.get" => Err((
            "unsupported".to_string(),
            "corral's embedded browser does not manage downloads/capture files yet. \
             Use browser.preview for files you created."
                .to_string(),
        )),
        "drag" => Err((
            "unsupported".to_string(),
            "browser.drag is not supported by corral's embedded browser yet. \
             Use click/fill, or select text and use keyboard actions."
                .to_string(),
        )),
        "trace.start" | "trace.stop" | "trace.analyze" | "cpu.start" | "cpu.stop"
        | "cpu.analyze" | "heap.snapshot" | "heap.summary" | "heap.query" | "heap.object"
        | "heap.compare" | "lighthouse" => Err((
            "unsupported".to_string(),
            format!(
                "browser.{kind} needs Chromium's devtools protocol. corral's embedded WebKit \
                 browser supports the page workflow (open, snapshot, click, fill, screenshot, \
                 console, network) but not profiling, heap analysis or Lighthouse."
            ),
        )),
        _ => Err(unsupported(kind)),
    }
}

fn with_tab(host: &Host, tab_id: &str, value: Value, keys: &[&str]) -> Result<Value, BrowserError> {
    let tab = host.window.tab_json(tab_id).ok_or_else(tab_unavailable)?;
    let object = value
        .as_object()
        .ok_or_else(|| js_error_str("browser agent returned a non-object result"))?;
    let mut out = serde_json::Map::new();
    out.insert("tab".to_string(), tab);
    for key in keys {
        if let Some(value) = object.get(*key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    Ok(Value::Object(out))
}

fn tab_id(action: &Value) -> Result<String, BrowserError> {
    action
        .get("tabID")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(tab_unavailable)
}

fn frame_supported(action: &Value) -> Result<(), BrowserError> {
    match action.get("frameID").and_then(Value::as_str) {
        Some(frame) if frame != "f0" && !frame.is_empty() => Err((
            "unsupported".to_string(),
            "corral's embedded browser currently operates on the main frame only. \
             Use browser.frames to list frames, but frame-targeted operations are not \
             available yet."
                .to_string(),
        )),
        _ => Ok(()),
    }
}

fn tab_unavailable() -> BrowserError {
    (
        "unsupported".to_string(),
        "corral does not have that browser tab. Call browser.tabs.list({}) and use an exact \
         returned tabID; tabs are closed when the user closes corral's browser window."
            .to_string(),
    )
}

fn js_error_str(message: impl Into<String>) -> BrowserError {
    ("unsupported".to_string(), message.into())
}

fn unsupported(kind: &str) -> BrowserError {
    (
        "unsupported".to_string(),
        format!("browser.{kind} is not supported by corral's browser host."),
    )
}

fn preview(host: &Host, raw: &str) -> Result<(), BrowserError> {
    let rel = resolve_preview_path(&host.worktree, raw).ok_or_else(|| {
        (
            "unsupported".to_string(),
            format!(
                "corral can only preview files inside this worktree; {raw} is outside it. \
                 Show the file from the worktree instead."
            ),
        )
    })?;
    let payload = PreviewEvent {
        project: host.project.clone(),
        feature: host.feature.clone(),
        path: rel.to_string_lossy().into_owned(),
    };
    if let Err(e) = host.app.emit("browser-preview", payload) {
        return Err((
            "unsupported".to_string(),
            format!("corral could not show {raw} in the File tab: {e}"),
        ));
    }
    Ok(())
}

/// Worktree-relative path for `raw` when it stays inside `worktree`. Absolute
/// paths are canonicalized (so symlinks can't escape), relative paths are
/// resolved lexically (the file may not exist yet), and any `..` that would
/// climb out is rejected.
fn resolve_preview_path(worktree: &Path, raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        let root = worktree.canonicalize().ok()?;
        let target = path.canonicalize().ok()?;
        return target.strip_prefix(&root).ok().map(Path::to_path_buf);
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        return None;
    }
    Some(out)
}

fn send_result(host: &Host, request_id: &str, outcome: Value) {
    if let Err(e) = opencode::browser_rpc(
        &host.base_url,
        &host.password,
        "result",
        &host.worktree,
        json!({
            "sessionID": host.session_id,
            "connectionID": host.connection_id,
            "requestID": request_id,
            "outcome": outcome,
        }),
    ) {
        tracing::debug!(
            "browser_host: failed to send result {request_id} for {}: {e}",
            host.id
        );
    }
}

// ---------------------------------------------------------------- UI commands
//
// These are called only by corral's local browser chrome page. They are
// granted to `corral-browser-chrome-*` webviews in
// `capabilities/browser.json`; page webviews get no capability at all.

fn host_for(state: &BrowserHosts, session: &str) -> Result<Arc<Host>, String> {
    state
        .get(session)
        .ok_or_else(|| format!("no browser host for session {session}"))
}

fn bounds(x: f64, y: f64, width: f64, height: f64) -> browser_window::Bounds {
    browser_window::clamp_bounds(browser_window::Bounds {
        x,
        y,
        width,
        height,
    })
}

#[tauri::command]
pub fn browser_ui_ready(
    state: State<BrowserHosts>,
    session: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let host = host_for(state.inner(), &session)?;
    host.window.ui_ready(bounds(x, y, width, height));
    Ok(())
}

#[tauri::command]
pub fn browser_ui_bounds(
    state: State<BrowserHosts>,
    session: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let host = host_for(state.inner(), &session)?;
    host.window.set_bounds(bounds(x, y, width, height));
    Ok(())
}

#[tauri::command]
pub fn browser_tab_new(
    state: State<BrowserHosts>,
    session: String,
    url: Option<String>,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let tab = host
        .window
        .open_tab(url.as_deref().unwrap_or("about:blank"), true)
        .map_err(|(_, message)| message)?;
    host.send_state();
    Ok(tab)
}

#[tauri::command]
pub fn browser_tab_focus(
    state: State<BrowserHosts>,
    session: String,
    tab_id: String,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let tab = host
        .window
        .focus_tab(&tab_id)
        .map_err(|(_, message)| message)?;
    host.send_state();
    Ok(tab)
}

#[tauri::command]
pub fn browser_tab_close(
    state: State<BrowserHosts>,
    session: String,
    tab_id: String,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let next = host
        .window
        .close_tab(&tab_id)
        .map_err(|(_, message)| message)?;
    host.send_state();
    Ok(next)
}

#[tauri::command]
pub fn browser_navigate(
    state: State<BrowserHosts>,
    session: String,
    url: String,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let focused = host
        .window
        .focused_tab_json()
        .and_then(|tab| tab.get("id").and_then(Value::as_str).map(str::to_string));
    let tab = match focused {
        Some(tab_id) => host
            .window
            .navigate(&tab_id, &url)
            .map_err(|(_, message)| message)?,
        None => host
            .window
            .open_tab(&url, true)
            .map_err(|(_, message)| message)?,
    };
    host.send_state();
    Ok(tab)
}

#[tauri::command]
pub fn browser_go_back(
    state: State<BrowserHosts>,
    session: String,
    tab_id: String,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let tab = host
        .window
        .go_back(&tab_id)
        .map_err(|(_, message)| message)?;
    host.send_state();
    Ok(tab)
}

#[tauri::command]
pub fn browser_go_forward(
    state: State<BrowserHosts>,
    session: String,
    tab_id: String,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let tab = host
        .window
        .go_forward(&tab_id)
        .map_err(|(_, message)| message)?;
    host.send_state();
    Ok(tab)
}

#[tauri::command]
pub fn browser_reload(
    state: State<BrowserHosts>,
    session: String,
    tab_id: String,
) -> Result<Value, String> {
    let host = host_for(state.inner(), &session)?;
    let tab = host
        .window
        .reload(&tab_id)
        .map_err(|(_, message)| message)?;
    host.send_state();
    Ok(tab)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_preview_path_keeps_relative_paths_inside_the_worktree() {
        let root = Path::new("/work/tree");
        assert_eq!(
            resolve_preview_path(root, "docs/shot.png").unwrap(),
            PathBuf::from("docs/shot.png")
        );
        assert_eq!(
            resolve_preview_path(root, "./docs/../README.md").unwrap(),
            PathBuf::from("README.md")
        );
    }

    #[test]
    fn resolve_preview_path_rejects_escaping_relative_paths() {
        let root = Path::new("/work/tree");
        assert!(resolve_preview_path(root, "../outside.png").is_none());
        assert!(resolve_preview_path(root, "a/../../outside.png").is_none());
        assert!(resolve_preview_path(root, "").is_none());
    }
}
