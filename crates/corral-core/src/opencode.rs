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
use uuid::Uuid;

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
    let body = match &model {
        Some(model) => serde_json::json!({
            "location": { "directory": directory.to_string_lossy() },
            "model": model,
        }),
        None => serde_json::json!({
            "location": { "directory": directory.to_string_lossy() },
        }),
    };
    let body: Value = http_agent()
        .post(&url)
        .header("Authorization", &basic_auth(password))
        .send_json(body)
        .map_err(|e| Error::Other(format!("opencode: create_session_v2 failed: {e}")))?
        .body_mut()
        .read_json()
        .map_err(|e| Error::Other(format!("opencode: create_session_v2: bad response: {e}")))?;
    body.pointer("/data/id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Other("opencode: create_session_v2 response missing data.id".into()))
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

/// Best-effort removal of every opencode2 session whose directory is
/// `directory` (itself or any subdirectory), so removing a worktree doesn't
/// leave its old agent conversations behind in opencode2's own session
/// store — a future worktree reusing the same feature name (and thus the
/// same path) would otherwise resurface them.
///
/// Spawns a short-lived `opencode2 serve`, drives its session API over
/// localhost (list, then delete each matching session), and kills the
/// server. Every step is best-effort: any failure (no `opencode2` binary,
/// server won't start, list/delete error) is logged at debug and swallowed,
/// matching the remove flow's other best-effort cleanup.
pub fn remove_sessions_for_directory(directory: &Path) {
    let Some(bin) = opencode2_bin() else {
        tracing::debug!("opencode cleanup: no opencode2 binary; skipping");
        return;
    };

    let Ok(port) = free_port() else {
        tracing::debug!("opencode cleanup: free_port failed; skipping");
        return;
    };
    let base_url = base_url(port);
    let password = Uuid::new_v4().to_string();

    let mut server = match Command::new(&bin)
        .args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("OPENCODE_PASSWORD", &password)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            tracing::debug!("opencode cleanup: failed to spawn {}: {e}", bin.display());
            return;
        }
    };

    if let Err(e) = wait_until_ready_with_password(&base_url, &password, Duration::from_secs(10)) {
        tracing::debug!("opencode cleanup: server at {base_url} not ready: {e}");
        let _ = server.kill();
        let _ = server.wait();
        return;
    }

    for session_id in list_session_ids_for_directory(&base_url, &password, directory) {
        if let Err(e) = delete_session(&base_url, &password, &session_id) {
            tracing::debug!("opencode cleanup: delete {session_id} failed: {e}");
        }
    }

    let _ = server.kill();
    let _ = server.wait();
}

/// Resolve the `opencode2` binary from settings (respecting
/// `opencode2_executable_path`), or `None` if it can't be found.
fn opencode2_bin() -> Option<std::path::PathBuf> {
    let settings = crate::settings::load().ok()?;
    let bin = if settings.opencode2_executable_path.trim().is_empty() {
        "opencode2"
    } else {
        settings.opencode2_executable_path.trim()
    };
    crate::bin::resolve_executable(bin)
}

/// List the session ids for `directory` (itself and any subdirectories) via
/// the session API. Best-effort: returns an empty list on any failure.
fn list_session_ids_for_directory(base_url: &str, password: &str, directory: &Path) -> Vec<String> {
    let prefix = directory.to_string_lossy().into_owned();
    let url = format!("{base_url}/api/session?limit=10000");
    let mut response = match http_agent()
        .get(&url)
        .header("Authorization", &basic_auth(password))
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("opencode cleanup: list sessions failed: {e}");
            return Vec::new();
        }
    };
    let body: Value = match response.body_mut().read_json() {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("opencode cleanup: list sessions: bad response: {e}");
            return Vec::new();
        }
    };
    body.get("data")
        .and_then(Value::as_array)
        .map(|sessions| {
            sessions
                .iter()
                .filter(|s| {
                    let dir = s
                        .pointer("/location/directory")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    dir == prefix || dir.starts_with(&format!("{prefix}/"))
                })
                .filter_map(|s| s.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Delete one session by id.
///
/// A `404` is treated as success, not an error: the desired end state is "this
/// session no longer exists", and a session can already be gone for reasons
/// outside this call — a retried worktree removal whose first cleanup already
/// deleted it, or opencode's own teardown racing the list→delete window. Only
/// genuine failures are surfaced (still best-effort at the call site).
fn delete_session(base_url: &str, password: &str, session_id: &str) -> Result<()> {
    let url = format!("{base_url}/api/session/{session_id}");
    match http_agent()
        .delete(&url)
        .header("Authorization", &basic_auth(password))
        .call()
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::StatusCode(404)) => Ok(()),
        Err(e) => Err(Error::Other(format!(
            "opencode: delete session {session_id} failed: {e}"
        ))),
    }
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

    /// The V2 tests below need the `opencode2` binary on PATH (same
    /// skip-don't-fail tradeoff as any test depending on an external CLI tool
    /// not every dev machine/CI runner has).
    fn opencode2_available() -> bool {
        Command::new("opencode2")
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
        let child = Command::new("opencode2")
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
        if !opencode2_available() {
            eprintln!("skipping: opencode2 not on PATH");
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
    fn delete_session_treats_a_missing_session_as_success() {
        if !opencode2_available() {
            eprintln!("skipping: opencode2 not on PATH");
            return;
        }
        let server = spawn_test_server_v2();
        let dir = tempfile::tempdir().unwrap();
        let id = super::create_session_v2(&server.base_url, &server.password, dir.path()).unwrap();

        // First delete genuinely removes it; the second 404s server-side.
        super::delete_session(&server.base_url, &server.password, &id).unwrap();
        // "Already gone" is the desired end state, not a failure — a retried
        // worktree removal hits exactly this and must not log an error.
        super::delete_session(&server.base_url, &server.password, &id).unwrap();
        // Same for an id that never existed at all.
        super::delete_session(&server.base_url, &server.password, "ses_does_not_exist").unwrap();
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
