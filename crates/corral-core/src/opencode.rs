//! Process/HTTP helpers for driving an `opencode serve` instance: corral
//! spawns one headless server per opencode session, then talks to it over its
//! local HTTP API instead of a bare CLI flag, since opencode's CLI has no way
//! to pre-assign a session id the way `claude --session-id` does.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};

/// The shared `ureq::Agent` for every opencode HTTP call except
/// [`wait_until_ready`]'s own short-poll loop (which needs much tighter
/// per-attempt timeouts — see below). ureq's *default* agent has no
/// response-read timeout at all, so a wedged `opencode serve` that accepts a
/// connection but never answers could otherwise hang a session start or a
/// worktree removal forever. Bounding connect + response-read keeps each call
/// finite.
fn http_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .build(),
        )
    })
}

/// Ask the OS for an ephemeral free TCP port, then release it immediately.
/// Small TOCTOU race (something else could grab it before `opencode serve`
/// binds) — acceptable for a local dev-tool port allocation, same tradeoff
/// any "let the OS pick, then reuse the number" pattern accepts.
pub fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

pub fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn wait_until_ready(base_url: &str, timeout: Duration) -> Result<()> {
    wait_until_ready_with_auth(base_url, None, timeout)
}

pub fn wait_until_ready_with_password(
    base_url: &str,
    password: &str,
    timeout: Duration,
) -> Result<()> {
    wait_until_ready_with_auth(base_url, Some(password), timeout)
}

fn wait_until_ready_with_auth(
    base_url: &str,
    password: Option<&str>,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let url = format!("{base_url}/session/status");
    // ureq's *default* agent has no response-read timeout at all, so a
    // single poll could block far longer than `timeout` if the server
    // accepts the TCP connection before it's actually ready to answer (seen
    // live: opencode can take several seconds past first-connect to finish
    // starting up before it responds). Cap each attempt short so the
    // deadline check below actually gets a chance to run.
    let poll_config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_millis(500)))
        .timeout_recv_response(Some(Duration::from_millis(500)))
        .build();
    let agent = ureq::Agent::new_with_config(poll_config);
    loop {
        let mut request = agent.get(&url);
        if let Some(password) = password {
            request = request.header("Authorization", &basic_auth(password));
        }
        if request.call().is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::Other(format!(
                "opencode server at {base_url} did not become ready within {timeout:?}"
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn basic_auth(password: &str) -> String {
    format!(
        "Basic {}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("opencode:{password}"),
        )
    )
}

pub fn create_session(base_url: &str) -> Result<String> {
    let url = format!("{base_url}/session");
    let body: Value = http_agent()
        .post(&url)
        .send_json(serde_json::json!({}))
        .map_err(|e| Error::Other(format!("opencode: create_session failed: {e}")))?
        .body_mut()
        .read_json()
        .map_err(|e| Error::Other(format!("opencode: create_session: bad response: {e}")))?;
    body.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Other("opencode: create_session response missing id".into()))
}

/// Create a fresh session on an opencode2 (V2) server scoped to `directory`,
/// returning the server-assigned session id (`ses_...`). Unlike legacy
/// opencode's [`create_session`], V2's create endpoint requires the server
/// password (basic auth) and takes the session's location up front — Corral
/// creates the session before spawning the TUI so it can inject onboarding
/// instructions (see [`put_instruction_entry`]) and then point the TUI at a
/// known id via `--session`.
///
/// Also passes a [`ModelRef`] resolved from opencode's own default (see
/// [`resolve_default_model`]): `POST /api/session` only sets a model when
/// asked, so omitting it leaves the freshly attached TUI on "no model
/// selected" — not the same default the TUI would have picked had it created
/// the session itself.
pub fn create_session_v2(base_url: &str, password: &str, directory: &Path) -> Result<String> {
    let url = format!("{base_url}/api/session");
    let model = resolve_default_model(directory);
    // `wait_until_ready` only proves the HTTP server accepts connections, and
    // V2 starts answering `/session/status` with its web UI *before* the
    // location service is ready. `POST /api/session` then answers
    // `503 {"code":"service_starting"}` for a brief window. Retry that one
    // status (and only that one) so a fast session start isn't a race.
    let mut body: Option<Value> = None;
    for attempt in 0..100 {
        match http_agent()
            .post(&url)
            .header("Authorization", &basic_auth(password))
            .send_json(body_value(&model, directory))
        {
            Ok(mut response) => {
                body = Some(response.body_mut().read_json().map_err(|e| {
                    Error::Other(format!("opencode: create_session_v2: bad response: {e}"))
                })?);
                break;
            }
            Err(ureq::Error::StatusCode(503)) if attempt < 99 => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(Error::Other(format!(
                    "opencode: create_session_v2 failed: {e}"
                )));
            }
        }
    }
    let body: Value = body.ok_or_else(|| {
        Error::Other("opencode: create_session_v2 failed: server still starting after 10s".into())
    })?;
    body.pointer("/data/id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Other("opencode: create_session_v2 response missing data.id".into()))
}

/// The `POST /api/session` body: the session's location plus opencode's
/// default model when one could be resolved. Rebuilt per retry (the request
/// builder is consumed) instead of cloning the whole `Value`.
fn body_value(model: &Option<ModelRef>, directory: &Path) -> Value {
    match model {
        Some(model) => serde_json::json!({
            "location": { "directory": directory.to_string_lossy() },
            "model": model,
        }),
        None => serde_json::json!({
            "location": { "directory": directory.to_string_lossy() },
        }),
    }
}

/// The model an opencode session is created with: a provider + model id (and
/// optional variant). Mirrors opencode's own `ModelRef` shape on
/// `POST /api/session`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelRef {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Resolve the model a fresh opencode2 session should start with, so corral's
/// up-front session creation doesn't leave the TUI on "no model selected".
/// Mirrors opencode2's own default-model precedence:
///
/// 1. the `model` string in opencode config — the project's `opencode.json`
///    overrides the global `~/.config/opencode/opencode.json`;
/// 2. the most-recently-used model from
///    `~/.local/state/opencode/model.json` (what the TUI defaults its picker
///    to).
///
/// Returns `None` when nothing determines a model, leaving the caller to
/// create the session model-less (opencode2's native default).
pub fn resolve_default_model(directory: &Path) -> Option<ModelRef> {
    config_model(directory)
        .as_deref()
        .and_then(parse_model_string)
        .or_else(recent_model)
}

/// The `model` string from opencode config, checking the project's
/// `opencode.json` before the global one.
fn config_model(directory: &Path) -> Option<String> {
    if let Some(model) = read_config_model(&directory.join("opencode.json")) {
        return Some(model);
    }
    let global = dirs::config_dir()?.join("opencode").join("opencode.json");
    read_config_model(&global)
}

fn read_config_model(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// `"provider/model"` → `ModelRef { provider_id: "provider", id: "model" }`;
/// a trailing `/variant` segment is optional. Malformed strings (missing the
/// slash, or an empty side) return `None` so the caller can fall through to
/// the next source.
fn parse_model_string(s: &str) -> Option<ModelRef> {
    let mut parts = s.splitn(3, '/');
    let provider_id = parts.next()?.trim().to_string();
    let id = parts.next()?.trim().to_string();
    let variant = parts
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    if provider_id.is_empty() || id.is_empty() {
        return None;
    }
    Some(ModelRef {
        id,
        provider_id,
        variant,
    })
}

/// The most-recently-used model from `~/.local/state/opencode/model.json`.
fn recent_model() -> Option<ModelRef> {
    let path = dirs::state_dir()?.join("opencode").join("model.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    model_from_recent_json(&value)
}

/// First entry of `model.json`'s `recent` array, with its variant looked up
/// from the sibling `variant` map (`"provider/model"` → variant id).
fn model_from_recent_json(value: &Value) -> Option<ModelRef> {
    let first = value.get("recent")?.as_array()?.first()?;
    let provider_id = first.get("providerID")?.as_str()?.to_string();
    let id = first.get("modelID")?.as_str()?.to_string();
    let variant = value
        .get("variant")
        .and_then(|v| v.get(format!("{provider_id}/{id}")))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(ModelRef {
        id,
        provider_id,
        variant,
    })
}

/// Inject a session-specific instruction entry into an opencode2 session —
/// V2's equivalent of `claude --append-system-prompt`: privileged, always-in-
/// scope context rendered as part of the session's initial instructions,
/// rather than a plain user message that only reaches the model once. `key`
/// must match `^[a-z0-9][a-z0-9._-]*$`; the value is capped at 8KB by the
/// server (larger values get a 413).
///
/// The endpoint moved under opencode2's `/api/experimental/` prefix (as of
/// 2.0.14); it previously sat at the unprefixed `/api/session/...` path. Both
/// are tried (experimental first) so corral keeps working across opencode2
/// versions: a `404` on the current path retries the legacy one, and only if
/// *both* paths 404 (e.g. the session is genuinely gone) does this error out.
pub fn put_instruction_entry(
    base_url: &str,
    password: &str,
    session_id: &str,
    key: &str,
    value: &str,
) -> Result<()> {
    // Current path first; an older server answers 404 there, which the loop
    // below turns into a retry on the legacy prefix.
    let mut not_found = Vec::new();
    for prefix in ["/api/experimental", "/api"] {
        let url = format!("{base_url}{prefix}/session/{session_id}/instructions/entries/{key}");
        match http_agent()
            .put(&url)
            .header("Authorization", &basic_auth(password))
            .send_json(serde_json::json!({ "value": value }))
        {
            Ok(_) => return Ok(()),
            Err(ureq::Error::StatusCode(404)) => not_found.push(format!("{prefix}/session/…")),
            Err(e) => {
                return Err(Error::Other(format!(
                    "opencode: put_instruction_entry failed: {e}"
                )));
            }
        }
    }
    Err(Error::Other(format!(
        "opencode: put_instruction_entry failed: 404 on every known path ({}) — \
         the opencode2 instructions API may have moved again",
        not_found.join(", ")
    )))
}

/// Whether an opencode2 session with `session_id` still exists on `base_url`.
/// Corral's resume path checks this before pointing the TUI at a stored id,
/// so a stale id (e.g. the opencode2 store was cleared out-of-band) falls
/// back to a fresh, re-briefed session rather than the TUI's own "Session not
/// found" toast silently starting an unbriefed one.
pub fn session_exists_v2(base_url: &str, password: &str, session_id: &str) -> Result<bool> {
    let url = format!("{base_url}/api/session/{session_id}");
    match http_agent()
        .get(&url)
        .header("Authorization", &basic_auth(password))
        .call()
    {
        Ok(_) => Ok(true),
        // `http_status_as_error` is ureq's default, so a 404 surfaces as a
        // `StatusCode` error rather than a `200`-shaped error body.
        Err(ureq::Error::StatusCode(404)) => Ok(false),
        Err(e) => Err(Error::Other(format!(
            "opencode: session_exists_v2 failed: {e}"
        ))),
    }
}

/// Seed a freshly created session with Corral's onboarding briefing
/// (`onboarding::system_prompt`), as a real message rather than a `system`
/// field — confirmed live against opencode 1.18.14 that `system` on
/// `POST /session/{id}/message` only affects the single request it's sent
/// with (a later message that never resends it has no access to it, even
/// mid-session), so it can't stand in for `claude --append-system-prompt`.
/// `noReply: true` stops this from producing a visible assistant turn, but
/// the message's `parts` text still becomes real prior conversation
/// history — confirmed live that a later turn (sent with no special
/// handling, exactly how `opencode attach` sends the user's real input)
/// still has full access to it. Only meant to be called once, right after
/// `create_session`, before the paired `attach` process is spawned and
/// starts making its own HTTP calls that corral no longer controls.
pub fn send_context(base_url: &str, session_id: &str, text: &str) -> Result<()> {
    let url = format!("{base_url}/session/{session_id}/message");
    http_agent()
        .post(&url)
        .send_json(serde_json::json!({
            "noReply": true,
            "parts": [{"type": "text", "text": text}],
        }))
        .map_err(|e| Error::Other(format!("opencode: send_context failed: {e}")))?;
    Ok(())
}

pub fn session_status(base_url: &str, id: &str) -> Result<Option<Value>> {
    let url = format!("{base_url}/session/status");
    let body: Value = http_agent()
        .get(&url)
        .call()
        .map_err(|e| Error::Other(format!("opencode: session_status failed: {e}")))?
        .body_mut()
        .read_json()
        .map_err(|e| Error::Other(format!("opencode: session_status: bad response: {e}")))?;
    Ok(body.get(id).cloned())
}

/// Best-effort removal of every opencode session whose directory is
/// `directory` (itself or any subdirectory), plus this directory's persistent
/// TUI tab strip, so removing a worktree doesn't leave its old agent
/// conversations behind in opencode's own session store — a future worktree
/// reusing the same feature name (and thus the same path) would otherwise
/// resurface them.
///
/// Uses opencode's own `session list` / `session delete` CLI rather than the
/// server's HTTP API: those are the supported session-management commands, and
/// they keep working across releases (the internal API paths corral used to
/// drive changed shape more than once). `--standalone` runs each command
/// against a short-lived private server backed by the shared session store, so
/// no external server needs to be running. `session list` reports every
/// top-level session in the current project (all worktrees of a repo share one
/// project), so the JSON is filtered down to `directory`; `session delete`
/// removes a session together with its child/subagent sessions.
///
/// Every step is best-effort: any failure (no binary, list/delete error) is
/// logged at debug and swallowed, matching the remove flow's other best-effort
/// cleanup. Runs before the directory itself is removed, since `session list`
/// runs with the worktree as its cwd.
pub fn remove_sessions_for_directory(directory: &Path) {
    match opencode_bin() {
        Some(bin) => {
            for session_id in list_session_ids_for_directory(&bin, directory) {
                if let Err(e) = delete_session(&bin, directory, &session_id) {
                    tracing::debug!("opencode cleanup: delete {session_id} failed: {e}");
                }
            }
        }
        None => tracing::debug!("opencode cleanup: no opencode binary; skipping session delete"),
    }

    // Runs even if the binary is missing: a leftover tab strip should still be
    // cleared when its worktree goes away.
    forget_tabs_for_directory(directory);
}

/// Resolve the `opencode` binary from settings (respecting
/// `opencode2_executable_path`, the setting name kept for backwards
/// compatibility), or `None` if it can't be found.
fn opencode_bin() -> Option<std::path::PathBuf> {
    let settings = crate::settings::load().ok()?;
    let bin = if settings.opencode2_executable_path.trim().is_empty() {
        "opencode"
    } else {
        settings.opencode2_executable_path.trim()
    };
    crate::bin::resolve_executable(bin)
}

/// Session ids whose directory is `directory` (itself or any subdirectory).
/// Pure so the JSON shape — the part most likely to drift between opencode
/// versions — is unit-tested without spawning a server.
fn session_ids_for_directory(list: &Value, directory: &Path) -> Vec<String> {
    list.as_array()
        .map(|sessions| {
            sessions
                .iter()
                .filter(|s| {
                    s.get("directory")
                        .and_then(Value::as_str)
                        .map(|dir| Path::new(dir).starts_with(directory))
                        .unwrap_or(false)
                })
                .filter_map(|s| s.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// List the session ids for `directory` via the CLI. Best-effort: returns an
/// empty list on any failure.
fn list_session_ids_for_directory(bin: &Path, directory: &Path) -> Vec<String> {
    let output = match Command::new(bin)
        .args([
            "session",
            "list",
            "--standalone",
            "--format",
            "json",
            "-n",
            "100000",
        ])
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            tracing::debug!(
                "opencode cleanup: `session list` exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Vec::new();
        }
        Err(e) => {
            tracing::debug!(
                "opencode cleanup: failed to run {} session list: {e}",
                bin.display()
            );
            return Vec::new();
        }
    };
    let list: Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(e) => {
            tracing::debug!("opencode cleanup: `session list` returned invalid JSON: {e}");
            return Vec::new();
        }
    };
    session_ids_for_directory(&list, directory)
}

/// Delete one session (and its child sessions) via the CLI.
fn delete_session(bin: &Path, directory: &Path, session_id: &str) -> Result<()> {
    let output = Command::new(bin)
        .args(["session", "delete", "--standalone", session_id])
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| Error::Other(format!("opencode: session delete {session_id}: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "opencode: session delete {session_id} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// Drop `directory`'s persistent opencode TUI tab-strip entry.
///
/// opencode v2 remembers open session tabs per working directory in
/// `$XDG_STATE_HOME/opencode/<channel>/tui/tabs.json` (`~/.local/state` by
/// default). Deleting a session from the store does not remove its tab, so
/// without this a worktree would reopen with tabs for conversations that no
/// longer exist — and a fresh corral session would show them next to the
/// session it just launched.
///
/// Called both before launching (so a new/resumed session starts with a clean
/// strip; the TUI adds the launched session back as the only tab) and when
/// removing a worktree. Best-effort: a missing or unreadable file is normal,
/// and a write failure is only logged.
///
/// Only the default `tabs.scope: "cwd"` layout is per-directory; a user who
/// switched opencode to `tabs.scope: "global"` shares one strip across every
/// project, and corral leaves those entries alone rather than guessing which
/// tabs belong to this worktree.
pub fn forget_tabs_for_directory(directory: &Path) {
    // `dirs::state_dir()` is Linux/Windows-only; opencode itself still uses
    // `~/.local/state` on macOS, so fall back to that.
    let Some(state) = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
    else {
        return;
    };
    let opencode_state = state.join("opencode");
    forget_tabs_in(&opencode_state, directory);
    // opencode stores the real path; a worktree reached through a symlinked
    // parent would otherwise not match.
    if let Ok(canonical) = directory.canonicalize()
        && canonical != directory
    {
        forget_tabs_in(&opencode_state, &canonical);
    }
}

/// Walk every opencode release channel under `<state>/opencode` (e.g.
/// `latest`, `beta`) and prune `directory` from each one's tab strip.
fn forget_tabs_in(opencode_state: &Path, directory: &Path) {
    let Ok(entries) = std::fs::read_dir(opencode_state) else {
        return;
    };
    for entry in entries.flatten() {
        let tabs_path = entry.path().join("tui").join("tabs.json");
        if tabs_path.is_file() {
            forget_tabs_in_file(&tabs_path, directory);
        }
    }
}

/// Remove the `cwd` entries for `directory` (and anything beneath it) from one
/// `tabs.json`, preserving every other project's tabs.
fn forget_tabs_in_file(tabs_path: &Path, directory: &Path) {
    let Ok(raw) = std::fs::read_to_string(tabs_path) else {
        return;
    };
    let Ok(mut tabs) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(cwd) = tabs.get_mut("cwd").and_then(Value::as_object_mut) else {
        return;
    };
    let before = cwd.len();
    cwd.retain(|key, _| !Path::new(key).starts_with(directory));
    if cwd.len() == before {
        return;
    }
    match serde_json::to_vec(&tabs) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(tabs_path, bytes) {
                tracing::debug!(
                    "opencode cleanup: failed to rewrite {}: {e}",
                    tabs_path.display()
                );
            }
        }
        Err(e) => tracing::debug!(
            "opencode cleanup: failed to serialize {}: {e}",
            tabs_path.display()
        ),
    }
}

/// A failed call to OpenCode's `experimental.browser` RPC (registered by the
/// built-in browser plugin, one per server location). Kept separate from the
/// generic [`Error`] because the host must distinguish a *retryable* startup
/// race (`rpc.unavailable`, the plugin still loading) from a real rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserRpcError {
    /// `rpc.unavailable`: the browser plugin's RPC is not registered yet (or
    /// has been unloaded). On a freshly started server this is expected for a
    /// few seconds while the plugin supervisor activates; retry it.
    Unavailable { message: String },
    /// Any other structured RPC error, e.g. `unavailable` for a detached
    /// connection or `rpc.invalid_input`.
    Rejected { kind: String, message: String },
    /// The HTTP request itself failed (server gone, connection reset, ...).
    Transport(String),
}

impl std::fmt::Display for BrowserRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { message } => write!(f, "rpc.unavailable: {message}"),
            Self::Rejected { kind, message } => write!(f, "{kind}: {message}"),
            Self::Transport(message) => f.write_str(message),
        }
    }
}

/// Normal RPC calls: bounded response time so a wedged server can't hang the
/// host thread. `http_status_as_error(false)` makes a structured error
/// response readable, which the default `ureq` agent would turn into a bare
/// status error.
fn browser_rpc_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .http_status_as_error(false)
                .build(),
        )
    })
}

/// `attach` does not return until the attachment closes (that is the RPC's
/// whole lifetime), so it must not have a response-read timeout.
fn browser_attach_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(None)
                .http_status_as_error(false)
                .build(),
        )
    })
}

fn browser_rpc_post(
    agent: &ureq::Agent,
    base_url: &str,
    password: &str,
    method: &str,
    directory: &Path,
    input: Value,
) -> std::result::Result<Value, BrowserRpcError> {
    let mut url = url::Url::parse(&format!("{base_url}/api/rpc/experimental.browser/{method}"))
        .map_err(|e| BrowserRpcError::Transport(format!("{method}: {e}")))?;
    // The server resolves the plugin instance by location. Passing the
    // session's directory explicitly keeps working when the session was
    // created through a slug symlink (the server's default location is the
    // canonical target, which is a different string).
    url.query_pairs_mut()
        .append_pair("location[directory]", &directory.to_string_lossy());
    let mut response = agent
        .post(url.as_str())
        .header("Authorization", &basic_auth(password))
        .send_json(serde_json::json!({ "input": input }))
        .map_err(|e| BrowserRpcError::Transport(format!("{method}: {e}")))?;
    let status = response.status();
    // Error bodies are structured (`{"_tag":"RpcError","type":...,"message":...}`);
    // a non-JSON body (e.g. an HTML error page) becomes a generic rejection.
    let body: Value = response.body_mut().read_json().unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body.get("output").cloned().unwrap_or(Value::Null));
    }
    let kind = body
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("http_error")
        .to_string();
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("opencode browser RPC failed")
        .to_string();
    if kind == "rpc.unavailable" {
        Err(BrowserRpcError::Unavailable { message })
    } else {
        Err(BrowserRpcError::Rejected { kind, message })
    }
}

/// Call one method of OpenCode's `experimental.browser` RPC. `input` is the
/// method's input object *without* the `{"input": ...}` wrapper; the return
/// value is the method's `output` (`Null` when the server sends none).
pub fn browser_rpc(
    base_url: &str,
    password: &str,
    method: &str,
    directory: &Path,
    input: Value,
) -> std::result::Result<Value, BrowserRpcError> {
    browser_rpc_post(
        browser_rpc_agent(),
        base_url,
        password,
        method,
        directory,
        input,
    )
}

/// Attach this process as the browser host for `session_id`. Blocks for the
/// attachment's lifetime, returning `"closed"` or `"replaced"` once the
/// server ends it. `connection_id` is the host's correlation handle; control
/// events carry it back over the event stream.
pub fn browser_attach(
    base_url: &str,
    password: &str,
    session_id: &str,
    connection_id: &str,
    directory: &Path,
) -> std::result::Result<String, BrowserRpcError> {
    let output = browser_rpc_post(
        browser_attach_agent(),
        base_url,
        password,
        "attach",
        directory,
        serde_json::json!({
            "sessionID": session_id,
            "connectionID": connection_id,
            "version": 4,
        }),
    )?;
    Ok(output.as_str().unwrap_or("closed").to_string())
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn free_port_returns_a_bindable_port() {
        // `free_port()`'s own doc comment already accepts a TOCTOU race
        // against the wider world: something else on the machine can grab
        // the port in the gap between `free_port()` releasing its listener
        // and this test's own verifying `bind()`. Confirmed live on this
        // (heavily loaded, many-concurrent-agents) dev machine: instrumented
        // runs showed `free_port_returns_a_bindable_port` losing that race
        // to *outside* processes even with the other `opencode::` test in
        // this module fully serialized against it (i.e. it's not the two
        // tests racing each other) — some other process on the box grabs the
        // freed port before the rebind. A single retry with a fresh port
        // absorbs that same-machine noise without masking a real regression
        // in `free_port()` itself (which would fail every retry, not just
        // occasionally).
        let mut last_err = None;
        for _ in 0..5 {
            let port = super::free_port().unwrap();
            match std::net::TcpListener::bind(("127.0.0.1", port)) {
                Ok(_) => return,
                Err(e) => last_err = Some(e),
            }
        }
        panic!("port from free_port() was not bindable after 5 attempts: {last_err:?}");
    }

    /// The V2 tests below need the `opencode` binary on PATH (same
    /// skip-don't-fail tradeoff as any test depending on an external CLI tool
    /// not every dev machine/CI runner has).
    fn opencode_available() -> bool {
        Command::new("opencode")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    struct TestServerV2 {
        child: std::process::Child,
        base_url: String,
        password: String,
        /// Kept alive so the isolated `OPENCODE_DB` file (and its dir) outlive
        /// the server. Without this, tests wrote their sessions straight into
        /// the developer's real `~/.local/share/opencode/opencode.db`.
        _db_dir: tempfile::TempDir,
    }

    impl Drop for TestServerV2 {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn spawn_test_server_v2() -> TestServerV2 {
        let port = super::free_port().unwrap();
        let base_url = super::base_url(port);
        let password = "corral-test-password".to_string();
        // Run the test server against a throwaway DB so it can't pollute the
        // developer's real opencode store with `/tmp/.tmpXXXX` sessions.
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = db_dir.path().join("opencode.db");
        let child = Command::new("opencode")
            .args([
                "serve",
                "--port",
                &port.to_string(),
                "--hostname",
                "127.0.0.1",
            ])
            .env("OPENCODE_PASSWORD", &password)
            .env("OPENCODE_DB", &db_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        super::wait_until_ready_with_password(&base_url, &password, Duration::from_secs(10))
            .unwrap();
        TestServerV2 {
            child,
            base_url,
            password,
            _db_dir: db_dir,
        }
    }

    #[test]
    fn create_session_v2_then_put_instruction_entry_round_trip() {
        if !opencode_available() {
            eprintln!("skipping: opencode not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let server = spawn_test_server_v2();
        let id = super::create_session_v2(&server.base_url, &server.password, dir.path()).unwrap();
        assert!(id.starts_with("ses_"));

        super::put_instruction_entry(
            &server.base_url,
            &server.password,
            &id,
            "corral-onboarding",
            "run `corral rename \"Fix Login Timeout\"` immediately",
        )
        .unwrap();

        // The entry must actually be stored and visible via the API (not just
        // acknowledged), since it's what the model reads back as instructions.
        // The listing endpoint shares the same `/api/experimental` prefix the
        // put helper targets first.
        let url = format!(
            "{}/api/experimental/session/{id}/instructions/entries",
            server.base_url
        );
        let body: serde_json::Value = super::http_agent()
            .get(&url)
            .header("Authorization", &super::basic_auth(&server.password))
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();
        let value = body
            .pointer("/data/0/value")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(value.contains("corral rename"));

        // The session the server just assigned exists; a made-up id does not
        // (drives the resume-vs-fresh decision in start_session).
        assert!(super::session_exists_v2(&server.base_url, &server.password, &id).unwrap());
        assert!(
            !super::session_exists_v2(&server.base_url, &server.password, "ses_does_not_exist",)
                .unwrap()
        );
    }

    #[test]
    fn session_ids_for_directory_matches_the_directory_and_its_children() {
        let list = serde_json::json!([
            { "id": "ses_here", "directory": "/w/proj/feat" },
            { "id": "ses_sub", "directory": "/w/proj/feat/crates/core" },
            { "id": "ses_sibling", "directory": "/w/proj/feat-2" },
            { "id": "ses_other", "directory": "/w/other/feat" },
            { "id": "ses_missing_dir" },
        ]);
        assert_eq!(
            super::session_ids_for_directory(&list, std::path::Path::new("/w/proj/feat")),
            vec!["ses_here".to_string(), "ses_sub".to_string()]
        );
        // A non-object payload (e.g. an error shape) is treated as empty, not a
        // panic.
        assert!(
            super::session_ids_for_directory(&serde_json::json!({}), std::path::Path::new("/w"))
                .is_empty()
        );
    }

    #[test]
    fn forget_tabs_removes_only_the_directory_entry() {
        let state = tempfile::tempdir().unwrap();
        let tui = state.path().join("latest/tui");
        std::fs::create_dir_all(&tui).unwrap();
        let tabs_path = tui.join("tabs.json");
        std::fs::write(
            &tabs_path,
            serde_json::json!({
                "global": { "tabs": [], "unread": {} },
                "cwd": {
                    "/gone/worktree": {
                        "tabs": [{ "sessionID": "ses_a", "title": "old" }],
                        "unread": {}
                    },
                    "/gone/worktree/sub": {
                        "tabs": [{ "sessionID": "ses_b", "title": "old sub" }],
                        "unread": {}
                    },
                    "/keep/worktree": {
                        "tabs": [{ "sessionID": "ses_c", "title": "keep" }],
                        "unread": {}
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        super::forget_tabs_in(state.path(), std::path::Path::new("/gone/worktree"));

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&tabs_path).unwrap()).unwrap();
        let cwd = value.get("cwd").unwrap().as_object().unwrap();
        assert!(!cwd.contains_key("/gone/worktree"));
        assert!(!cwd.contains_key("/gone/worktree/sub"));
        assert!(cwd.contains_key("/keep/worktree"));
    }

    #[test]
    fn parse_model_string_splits_provider_and_model() {
        let m = super::parse_model_string("deepseek/deepseek-v4-pro").unwrap();
        assert_eq!(
            m,
            super::ModelRef {
                id: "deepseek-v4-pro".into(),
                provider_id: "deepseek".into(),
                variant: None,
            }
        );
    }

    #[test]
    fn parse_model_string_accepts_a_variant_segment() {
        let m = super::parse_model_string("openai/gpt-5.6-fast/medium").unwrap();
        assert_eq!(m.provider_id, "openai");
        assert_eq!(m.id, "gpt-5.6-fast");
        assert_eq!(m.variant.as_deref(), Some("medium"));
    }

    #[test]
    fn parse_model_string_rejects_malformed() {
        assert!(super::parse_model_string("no-slash").is_none());
        assert!(super::parse_model_string("/only-provider").is_none());
        assert!(super::parse_model_string("provider/").is_none());
        assert!(super::parse_model_string("").is_none());
    }

    #[test]
    fn model_from_recent_json_reads_first_entry_and_variant() {
        let value = serde_json::json!({
            "recent": [
                { "providerID": "deepseek", "modelID": "deepseek-v4-pro" },
                { "providerID": "openai", "modelID": "gpt-5.6-fast" }
            ],
            "variant": {
                "deepseek/deepseek-v4-pro": "high",
                "openai/gpt-5.6-fast": "medium"
            }
        });
        let m = super::model_from_recent_json(&value).unwrap();
        assert_eq!(
            m,
            super::ModelRef {
                id: "deepseek-v4-pro".into(),
                provider_id: "deepseek".into(),
                variant: Some("high".into()),
            }
        );
    }

    #[test]
    fn model_from_recent_json_returns_none_when_empty() {
        assert!(super::model_from_recent_json(&serde_json::json!({})).is_none());
        assert!(super::model_from_recent_json(&serde_json::json!({ "recent": [] })).is_none());
    }
}
