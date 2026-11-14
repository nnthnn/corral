//! Agent sessions: a `claude` process running in a PTY inside a worktree.
//!
//! Each worktree hosts at most one session, keyed by `<project>/<feature>`.
//! Output is streamed to the frontend via `session-output` events and also
//! retained in a capped scrollback buffer so a terminal can re-attach with
//! history. Keystrokes flow back in via [`write_session`].

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use corral_core::config;

use super::StrErr;

/// How long an exited (not explicitly closed) session stays in the registry
/// before `list_sessions` sweeps it up. Generous — this only guards against
/// unbounded growth from sessions nobody ever dismisses, not a UX affordance,
/// so it shouldn't fire while someone's still looking at the exited output.
const STALE_EXITED_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Default)]
struct Inner {
    /// Recent raw PTY output, capped at the session's `max_buffer`.
    buffer: Vec<u8>,
    running: bool,
    exit_code: Option<i32>,
    /// True if we asked the process to stop (vs. it exiting on its own).
    killed: bool,
    /// When the process exited on its own or was killed; `None` while
    /// `running`. Drives `list_sessions`' pruning of stale exited entries.
    exited_at: Option<Instant>,
    /// Whether the agent is genuinely waiting on the user, per Claude Code's
    /// own hooks (`Stop` / idle `Notification` → true, `UserPromptSubmit` →
    /// false) — see `corral_core::hooks`. A fresh session starts `true`: a
    /// newly spawned `claude` process is sitting at its own input prompt,
    /// which is a real "waiting for input" state.
    waiting: bool,
    /// When `waiting` last flipped `false -> true`; `None` while not waiting.
    /// Drives idle-timeout auto-stop (`idle_timeout::spawn`'s periodic
    /// sweep) — a session sitting continuously "waiting" longer than
    /// `Settings.idle_timeout_minutes` gets auto-stopped.
    waiting_since: Option<Instant>,
    /// One-line-ish summary of the assistant's last completed turn, parsed
    /// from the `Stop` hook's transcript by `corral-cli` — in-memory only,
    /// not persisted, and reset on process restart just like `waiting`. Left
    /// unchanged by `Notification`/`UserPromptSubmit` hooks and by a `Stop`
    /// whose transcript couldn't be parsed, so it keeps showing the last
    /// known summary rather than flickering to empty.
    last_summary: Option<String>,
}

/// A live (or exited) agent session.
pub struct Session {
    id: String,
    project: String,
    feature: String,
    /// PTY master, for `resize_session`.
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// PTY master writer, for `write_session`.
    writer: Mutex<Box<dyn Write + Send>>,
    /// The primary child process (`claude`, `opencode attach`, or the
    /// `opencode` TUI).
    child: Mutex<Box<dyn Child + Send + Sync>>,
    /// The headless `opencode serve` process backing this session, when
    /// `harness` is Opencode. `None` for Claude sessions, which have no
    /// paired process. Typed the same as `child` (`Box<dyn Child + Send +
    /// Sync>`) rather than a raw `std::process::Child` or a `UnixChild`
    /// wrapper: `portable_pty` 0.9.0 implements its `Child`/`ChildKiller`
    /// traits directly for `std::process::Child` (see
    /// `portable_pty::lib::impl Child for std::process::Child`) — there's no
    /// `UnixChild` conversion type in this version at all, and none is needed
    /// since the trait impl already covers a plain `std::process::Child` that
    /// was never attached to a PTY. Reusing the same field type as `child`
    /// also means `kill()` below can treat both uniformly.
    opencode_server: Option<Mutex<Box<dyn Child + Send + Sync>>>,
    /// Unique marker (`CORRAL_OC2_TOKEN=<token>`) passed via the environment
    /// to a spawned `opencode` TUI, so the detached `serve
    /// --stdio` child that TUI re-parents can be found and killed via /proc
    /// once the session ends. `Some` only for `Harness::Opencode2` sessions.
    opencode2_token: Option<String>,
    inner: Mutex<Inner>,
    /// Cap on retained scrollback (bytes), from `Settings.scrollback_kb` at
    /// launch time.
    max_buffer: usize,
    /// Whether this spawn used a resumed session id rather than starting a
    /// brand new conversation — drives the `resume_failed` check on exit.
    resumed: bool,
}

/// Whether a hook transitioning `waiting` from `was_waiting` to `new_waiting`
/// should (re)start the idle-timeout clock. Only a `false -> true` edge
/// starts a fresh clock; a repeated `true -> true` hook (e.g. two
/// `Notification` hooks in a row) leaves the existing clock untouched, and
/// any transition to `false` clears it.
fn next_waiting_since(
    current: Option<Instant>,
    was_waiting: bool,
    new_waiting: bool,
) -> Option<Instant> {
    if new_waiting && !was_waiting {
        Some(Instant::now())
    } else if new_waiting {
        current
    } else {
        None
    }
}

impl Session {
    fn info(&self) -> SessionInfo {
        let inner = self.inner.lock();
        SessionInfo {
            id: self.id.clone(),
            project: self.project.clone(),
            feature: self.feature.clone(),
            running: inner.running,
            exit_code: inner.exit_code,
            killed: inner.killed,
            waiting: inner.waiting,
            last_summary: inner.last_summary.clone(),
        }
    }

    /// Applied by the hook-callback socket listener when a `Stop`/
    /// `Notification`/`UserPromptSubmit` hook fires for this session.
    /// `summary` is only ever `Some` from a `Stop` hook whose transcript
    /// parsed cleanly — a `None` here leaves the previous summary in place.
    fn apply_hook(&self, waiting: bool, summary: Option<String>) -> SessionInfo {
        let mut inner = self.inner.lock();
        inner.waiting_since = next_waiting_since(inner.waiting_since, inner.waiting, waiting);
        inner.waiting = waiting;
        if let Some(s) = summary {
            inner.last_summary = Some(s);
        }
        drop(inner);
        self.info()
    }

    /// Best-effort kill of the paired `opencode serve` process, if any.
    /// Idempotent and safe to call on an already-exited (or already-killed)
    /// server — `ChildKiller::kill` on a dead process is harmless, it just
    /// fails silently, which is fine since nothing here depends on the
    /// result. Called from every path that can end a session's life: the
    /// explicit `kill()` below, `spawn_reader`'s natural-exit handling (the
    /// primary `child` — `opencode attach` — can exit on its own, e.g. it
    /// crashed, the user quit the TUI, or it lost its connection to the
    /// paired server), and `Session`'s `Drop` impl (defense in depth for any
    /// other path that drops the last `Arc<Session>`, e.g. `close_session`,
    /// without either of the above having run first).
    fn kill_opencode_server(&self) {
        if let Some(server) = &self.opencode_server {
            let _ = server.lock().kill();
        }
    }

    /// Best-effort sweep-and-kill of every process still carrying this
    /// session's `CORRAL_OC2_TOKEN` marker — the `opencode`
    /// TUI's detached `serve --stdio` child (re-parented and abandoned by
    /// the TUI, so it outlives both the TUI and the node shim) and, if the
    /// TUI itself survived `child.kill()` above, the TUI process too.
    /// Idempotent: once the marker has no living carriers this is a no-op.
    /// `None` for non-Opencode2 sessions.
    fn sweep_opencode2_markers(&self) {
        if let Some(token) = &self.opencode2_token {
            kill_opencode2_children(token);
        }
    }

    /// Mark the process as deliberately stopped and kill it — shared by the
    /// manual `kill_session` command and the idle-timeout sweep, so an
    /// auto-stopped session's exit looks identical to a user-clicked Stop
    /// (both surface `killed: true` in the `session-exit` event the reader
    /// thread already emits).
    ///
    /// Checks `try_wait()` under the same `child` lock used for the actual
    /// kill, instead of setting `inner.killed` first: the reader thread's
    /// exit handling (`session.child.lock().wait()`, then locking `inner` to
    /// compute `resume_failed`) blocks on this same lock, so holding it here
    /// while probing exit status closes the window where a session that
    /// already crashed on its own (e.g. a `--resume` failure) between the
    /// sweep's `running` check and this call would otherwise get `killed`
    /// stamped `true` first — masking `resume_failed` and skipping the
    /// frontend's auto-fallback-to-a-fresh-session recovery.
    ///
    /// `kill_opencode_server()` runs unconditionally, *before* the
    /// already-exited early return below — not gated behind it. An opencode
    /// session's paired `serve` process is independent of whether the
    /// visible `attach` PTY process happened to still be alive when this was
    /// called; if `attach` already exited on its own (crash, the user
    /// quitting the TUI, losing its connection to `serve`, ...) the server
    /// would otherwise never get killed by this method at all, leaking it
    /// indefinitely from the most common real-world exit path.
    fn kill(&self) -> std::io::Result<()> {
        self.kill_opencode_server();
        let result = {
            let mut child = self.child.lock();
            if matches!(child.try_wait(), Ok(Some(_))) {
                // Already exited on its own; let the reader thread's own
                // resume_failed detection (and its opencode2 marker sweep)
                // run untouched.
                return Ok(());
            }
            self.inner.lock().killed = true;
            child.kill()
        };
        // Only reached when the primary `child` was still alive and got
        // killed above. For Opencode2 sessions, killing the TUI's node shim
        // does *not* kill the detached `serve --stdio` it spawned (that
        // process is designed to survive its parent), so sweep it up here;
        // the already-exited path above is covered by the reader thread's
        // own sweep on natural exit.
        self.sweep_opencode2_markers();
        result
    }
}

impl Drop for Session {
    /// Defense in depth for [`Session::kill_opencode_server`] (and, for
    /// Opencode2 sessions, the marker sweep): catches any path that drops
    /// the last `Arc<Session>` without having gone through `kill()` or
    /// `spawn_reader`'s natural-exit handling first — currently
    /// `close_session` (which just removes the registry entry with no kill
    /// call of its own) and `prune_stale_exited_sessions`. Both of those
    /// already-safe paths only run once the primary `child` is confirmed
    /// exited, so this is a backstop, not the primary mechanism.
    fn drop(&mut self) {
        self.kill_opencode_server();
        self.sweep_opencode2_markers();
    }
}

/// SessionRegistry of sessions, held as Tauri managed state.
#[derive(Default)]
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Ids currently mid-spawn in [`start_session`], reserved for the
    /// duration of the (slow) PTY spawn so a second concurrent call for the
    /// same worktree — a double-click, or a race with an auto-resume call —
    /// can't slip past the "not already running" check and spawn a second
    /// real `claude` process before the first has been inserted into
    /// `sessions`.
    starting: Mutex<std::collections::HashSet<String>>,
}

/// Releases a [`SessionRegistry::starting`] reservation when dropped, so it's
/// freed on every `start_session` exit path (success, `?`-propagated error,
/// or early return) without repeating the cleanup at each one.
struct StartGuard<'a> {
    registry: &'a SessionRegistry,
    id: String,
}

impl Drop for StartGuard<'_> {
    fn drop(&mut self) {
        self.registry.starting.lock().remove(&self.id);
    }
}

/// Kills the wrapped `opencode serve` child if this guard is dropped while
/// still holding one. `start_session`'s opencode arm spawns `serve` before
/// several more fallible steps (`wait_until_ready`, `create_session`, and —
/// shared with the Claude path — the final PTY `spawn_command` for `opencode
/// attach`); any of those returning early via `?` would otherwise drop a
/// bare `Box<dyn Child>` with nothing left holding its handle. Dropping a
/// `std::process::Child` does *not* kill the process (a well-known Rust
/// footgun), so without this guard a failed spawn would leak a running
/// `opencode serve` with no PTY/attach process left to pair it with. Once
/// the whole spawn succeeds, `take()` moves the child out into the
/// constructed `Session` and leaves the guard holding `None`, so its own
/// drop at function-end is a no-op.
#[derive(Default)]
struct KillServerOnDrop(Option<Box<dyn Child + Send + Sync>>);

impl KillServerOnDrop {
    fn take(&mut self) -> Option<Box<dyn Child + Send + Sync>> {
        self.0.take()
    }
}

impl Drop for KillServerOnDrop {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
        }
    }
}

impl SessionRegistry {
    /// Update a session's waiting status (and, optionally, its last-turn
    /// summary) by id, returning its fresh `SessionInfo` for the caller to
    /// broadcast — or `None` if no such session is registered (e.g. a stale
    /// hook message after the session closed).
    pub fn apply_hook(
        &self,
        id: &str,
        waiting: bool,
        summary: Option<String>,
    ) -> Option<SessionInfo> {
        let sessions = self.sessions.lock();
        sessions.get(id).map(|s| s.apply_hook(waiting, summary))
    }

    /// Kill every session that's been continuously "waiting" for at least
    /// `timeout` — called periodically by `idle_timeout::spawn` when
    /// `Settings.idle_timeout_enabled` is on. Best-effort: a session whose
    /// kill fails is skipped (not retried) and simply absent from the
    /// returned list; returns the ids of sessions actually stopped, for
    /// logging.
    pub fn sweep_idle(&self, timeout: Duration) -> Vec<String> {
        let sessions = self.sessions.lock();
        sessions
            .iter()
            .filter(|(_, session)| {
                let inner = session.inner.lock();
                should_idle_stop(inner.running, inner.waiting_since, timeout)
            })
            .filter_map(|(id, session)| session.kill().ok().map(|()| id.clone()))
            .collect()
    }
}

/// Serializable session summary for the frontend.
#[derive(Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub project: String,
    pub feature: String,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub killed: bool,
    pub waiting: bool,
    pub last_summary: Option<String>,
}

/// Returned by [`attach_session`]: current state plus scrollback.
#[derive(Clone, Serialize)]
pub struct Attach {
    pub info: SessionInfo,
    pub buffer: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct OutputEvent {
    id: String,
    #[serde(with = "super::base64_bytes")]
    data: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct ExitEvent {
    id: String,
    code: Option<i32>,
    killed: bool,
    /// True when this session was started with `--resume` and claude exited
    /// on its own because the resumed conversation no longer exists (a stale
    /// session id) — the classic "No conversation found with session ID:
    /// ..." message. Retrying `--resume` again would just loop forever, so
    /// the frontend uses this to fall back to a fresh session automatically
    /// instead of showing the manual "Restart" button.
    resume_failed: bool,
}

/// The exact message claude's CLI prints (then exits 1) when `--resume
/// <id>` points at a session it can no longer find.
const RESUME_FAILURE_MARKER: &[u8] = b"No conversation found with session ID";

/// Whether `needle` occurs anywhere in `haystack`. `[u8]` has no built-in
/// substring search (unlike `str`), so this is a small manual scan — fine at
/// scrollback-buffer size (capped at a few MB) and called once per exit.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Env-var name for the per-session marker passed to a spawned
/// `opencode` TUI. Inherited by every process the TUI spawns —
/// including the detached `serve --stdio` it re-parents and abandons when
/// the TUI (or corral) dies — so [`kill_opencode2_children`] can find and
/// kill exactly the processes belonging to one session, regardless of how
/// the process tree got torn down.
const OPENCODE2_MARKER: &str = "CORRAL_OC2_TOKEN";

/// SIGKILL every process whose environment carries the given session's
/// `CORRAL_OC2_TOKEN` marker. `opencode` spawns a detached
/// `serve --stdio` child to host its backend; that child is `setsid`'d and
/// deliberately survives the death of its parent (it re-parents to init), so
/// killing the visible TUI process is not enough — the server would leak
/// forever. The marker lets corral find the exact processes to kill even
/// though no parent-child relationship survives.
///
/// Linux-specific: scans /proc. Best-effort: a process that dies (or becomes
/// unreadable) between the scan and the kill is simply skipped. Idempotent
/// once no marker-bearing processes remain, so it's safe to call from every
/// session-ending path (`Session::kill`, the reader thread's natural-exit
/// handling, and `Session::drop`) as a backstop.
fn kill_opencode2_children(token: &str) {
    let needle = format!("{OPENCODE2_MARKER}={token}");
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        // Environ entries are NUL-separated; `/proc/<pid>/environ` of a
        // zombie reads back empty, which simply matches nothing.
        let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
            continue;
        };
        let marked = environ.split(|b| *b == 0).any(|kv| kv == needle.as_bytes());
        if marked {
            // `kill -9` rather than a libc call: src-tauri doesn't depend on
            // libc directly, and `kill` is POSIX-required.
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .status();
        }
    }
}

/// A fresh `CORRAL_OC2_TOKEN` value unique to this session: the registry id
/// plus the corral process id and a nanosecond timestamp. Uniqueness only
/// has to beat accidental collisions between concurrently running corral
/// instances (each worktree has its own), which this comfortably does.
fn new_opencode2_token(id: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("corral-{id}-{}-{nanos}", std::process::id())
}

#[tauri::command]
pub fn list_sessions(registry: State<SessionRegistry>) -> Vec<SessionInfo> {
    prune_stale_exited_sessions(&registry, STALE_EXITED_RETENTION);
    registry
        .sessions
        .lock()
        .values()
        .map(|s| s.info())
        .collect()
}

/// Drop registry entries for sessions that exited more than `retention` ago
/// and were never explicitly dismissed via `close_session` — otherwise a
/// session whose process exits on its own just sits in the map forever.
/// Called from `list_sessions`, which the frontend already polls, so this
/// needs no dedicated background timer.
fn prune_stale_exited_sessions(registry: &SessionRegistry, retention: Duration) {
    registry.sessions.lock().retain(|_, session| {
        let exited_at = session.inner.lock().exited_at;
        !is_stale_exited(exited_at, retention)
    });
}

/// Whether a session that exited at `exited_at` (`None` if still running)
/// is old enough to be pruned. Split out from [`prune_stale_exited_sessions`]
/// so the age comparison is testable without a full PTY-backed `Session`.
fn is_stale_exited(exited_at: Option<Instant>, retention: Duration) -> bool {
    exited_at.is_some_and(|exited_at| exited_at.elapsed() > retention)
}

/// Whether a session that's been waiting since `waiting_since` (`None` if
/// not currently waiting) has been waiting at least `timeout` — but only if
/// it's still `running`. A session that already exited (crash or explicit
/// stop) must never be re-killed by the idle sweep just because it has a
/// stale `waiting_since` left over from before it exited; the reader thread
/// clears `running` on exit but doesn't touch `waiting_since`. Split out
/// like `is_stale_exited` above so it's testable without a real session.
fn should_idle_stop(running: bool, waiting_since: Option<Instant>, timeout: Duration) -> bool {
    running && waiting_since.is_some_and(|since| since.elapsed() >= timeout)
}

/// Cap `kb` (a user-configured `Settings.scrollback_kb` value) to a sane
/// 64KB-2048KB range and convert to bytes. Defensive against a hand-edited
/// `settings.json` with an out-of-range value; the Settings UI already
/// clamps 64-2048 before it gets here.
fn scrollback_bytes(kb: u32) -> usize {
    (kb.clamp(64, 2048) as usize) * 1024
}

/// Recompute whether any session is actively working and reconcile the sleep
/// inhibitor against `settings.caffeinate_enabled && any_busy`. Called after
/// every session start/exit, after every hook/SSE status transition, and
/// after settings are saved, so the inhibitor never lags the real state for
/// long.
///
/// "Busy" is `running && !waiting`: a session that's merely alive but idle
/// (waiting on the user, per Claude Code hooks / opencode SSE) does *not*
/// keep the machine awake — the whole point of caffeine is to prevent sleep
/// mid-task, not while an agent is parked at its prompt.
pub(crate) fn sync_caffeinate(app: &AppHandle) {
    let registry = app.state::<SessionRegistry>();
    let any_busy = registry.sessions.lock().values().any(|s| {
        let inner = s.inner.lock();
        inner.running && !inner.waiting
    });
    let settings = corral_core::settings::load().unwrap_or_default();
    app.state::<corral_core::caffeinate::InhibitGuard>()
        .sync(settings.caffeinate_enabled && any_busy);
}

// `async` here isn't about awaiting anything inside — the body stays plain
// blocking code throughout. It changes how Tauri *dispatches* this command:
// a non-async `#[tauri::command]` runs inline on whatever thread handles the
// IPC message (in practice, the app's main UI thread — confirmed by reading
// the vendored tauri-patched macro's `body_blocking` codegen), so the
// Opencode arm's up-to-10s blocking wait for `opencode serve` to become
// ready froze the entire app (every window, every other command) for that
// whole duration, not just this one session's UI. `async fn` instead gets
// dispatched via `respond_async_serialized`, which spawns the whole call
// onto Tauri's own async runtime (a separate thread pool) and returns
// immediately — the main thread stays free the whole time. The Claude arm's
// already-fast path is unaffected either way.
#[tauri::command]
pub async fn start_session(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    project: String,
    feature: String,
    fresh: Option<bool>,
) -> Result<SessionInfo, String> {
    let id = corral_core::agent_session::key(&project, &feature);

    // Reuse an existing live session for this worktree.
    if let Some(existing) = registry.sessions.lock().get(&id)
        && existing.inner.lock().running
    {
        return Ok(existing.info());
    }

    // Reserve `id` for the rest of this call, so a second concurrent
    // `start_session` for the same worktree can't race past the check above
    // and spawn a second real `claude` process while this one is still
    // spawning. Released automatically (success or error) when `_guard` drops.
    if !registry.starting.lock().insert(id.clone()) {
        return Err(format!("session for {id} is already starting"));
    }
    let _guard = StartGuard {
        registry: &registry,
        id: id.clone(),
    };

    let path = config::worktree_path(&project, &feature).str_err()?;
    if !path.exists() {
        return Err(format!("worktree path does not exist: {}", path.display()));
    }

    // `.claude/settings.local.json` is symlinked from every worktree back to
    // the main repo (`worktree::link_claude_dir`), so this is written once
    // per project and is safe to call on every session start — it's a
    // no-op once the hooks are already present (see `hooks::merge_hooks`).
    let repo_claude_settings = config::repo_path(&project)
        .str_err()?
        .join(".claude")
        .join("settings.local.json");
    if let Err(e) = corral_core::hooks::ensure_hooks_installed(&repo_claude_settings) {
        tracing::warn!("start_session: failed to install status hooks: {e}");
    }

    let settings = corral_core::settings::load().str_err()?;
    let project_settings = corral_core::project_settings::get(&project).str_err()?;

    // A non-empty saved title means this worktree was already named (via
    // `corral rename`, by this or a previous session). Passed to the
    // onboarding briefing so a fresh session isn't told to rename an
    // already-named worktree — otherwise a worktree used for a stream of
    // small fixes gets renamed anew on every session/task. The title lives
    // under the stable physical `feature` key, which survives renames.
    let existing_title = corral_core::worktree_settings::get_title(&id).unwrap_or_else(|e| {
        tracing::warn!("start_session: failed to read worktree title: {e}");
        String::new()
    });

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .str_err()?;

    // Resume the worktree's last session unless starting fresh, so
    // closing/reopening a worktree (or restarting corral) continues the same
    // conversation instead of losing it. The stored session is tagged with
    // the harness that created it (`StoredSession::harness`); switching
    // `settings.harness` between visits to the same worktree is treated the
    // same as `fresh` for that harness — there's no cross-harness resume, so
    // a stale id from the other harness is just ignored (`.filter(...)`
    // below) rather than fed to the wrong CLI.
    if fresh.unwrap_or(false) {
        corral_core::agent_session::remove(&id).str_err()?;
    }
    let stored = if fresh.unwrap_or(false) {
        None
    } else {
        corral_core::agent_session::get(&id).str_err()?
    };

    // Populated for the harnesses that use a separate headless server
    // (`opencode serve`, or V2's TCP-backed server).
    // backing the session. Kills itself if a later step fails before it's
    // handed off to the `Session` — see `KillServerOnDrop`.
    // `resumed` (used for the Claude-only `resume_failed` exit check) stays
    // meaningful for opencode too — it just means "resumed vs. brand-new
    // session id" rather than "claude --resume".
    let mut opencode_server_guard = KillServerOnDrop::default();
    // Used after the `Session` is constructed and registered to kick off the
    // event listener, which needs the server's base URL.
    let mut opencode_base_url: Option<String> = None;
    // Populated for both opencode harnesses: opencode's own root session id
    // (`ses_...`), distinct from `id` (corral's `<project>/<feature>` registry
    // key). `opencode_events::spawn` uses it to tell the root turn apart from
    // the child sessions opencode creates for subagents.
    let mut opencode_session_id: Option<String> = None;
    let mut opencode_password: Option<String> = None;
    // Only populated for `Harness::Opencode2`: the unique `CORRAL_OC2_TOKEN`
    // marker value passed to the `opencode` TUI (set in that
    // arm below), used later to sweep up its detached `serve --stdio` child
    // on session end.
    let mut opencode2_token: Option<String> = None;
    let resumed;

    // The PTY child command, built in the Claude/Opencode/Opencode2 arms
    // below; every harness now embeds its TUI in a real PTY. Every non-early-
    // returning arm assigns it, so no initializer is needed.
    let mut pty_cmd;
    match &settings.harness {
        corral_core::harness::Harness::Unknown(name) => {
            // A harness value written to settings.json by a newer corral
            // build than this one. The settings still load (see
            // `Harness::Unknown`) — it's *launching* that's impossible here —
            // so say so clearly instead of silently running a different agent.
            return Err(format!(
                "harness \"{name}\" (from settings.json) isn't supported by this \
                 corral build — update corral, or pick a supported harness under \
                 Settings"
            ));
        }
        corral_core::harness::Harness::Claude => {
            let claude_bin = if settings.claude_executable_path.trim().is_empty() {
                "claude"
            } else {
                settings.claude_executable_path.trim()
            };
            if corral_core::bin::resolve_executable(claude_bin).is_none() {
                return Err(corral_core::bin::missing_executable_error(
                    corral_core::harness::Harness::Claude,
                    claude_bin,
                ));
            }
            let mut cmd = CommandBuilder::new(claude_bin);
            let claude_stored = stored
                .as_ref()
                .filter(|s| s.harness == corral_core::harness::Harness::Claude);
            match claude_stored {
                Some(s) => {
                    resumed = true;
                    cmd.args(["--resume", &s.session_id]);
                }
                None => {
                    resumed = false;
                    let claude_id = corral_core::agent_session::new_id();
                    corral_core::agent_session::set(
                        &id,
                        corral_core::harness::Harness::Claude,
                        &claude_id,
                    )
                    .str_err()?;
                    cmd.args(["--session-id", &claude_id]);
                    let prompt = corral_core::onboarding::system_prompt(
                        &project,
                        &feature,
                        &existing_title,
                        &settings.extra_instructions,
                        settings.co_author_trailer,
                        &project_settings.project_prompt,
                    );
                    cmd.args(["--append-system-prompt", &prompt]);
                }
            }
            if settings.skip_permissions {
                cmd.arg("--dangerously-skip-permissions");
            }
            pty_cmd = Some(cmd);
        }
        corral_core::harness::Harness::Opencode => {
            let opencode_bin = if settings.opencode_executable_path.trim().is_empty() {
                "opencode"
            } else {
                settings.opencode_executable_path.trim()
            };
            if corral_core::bin::resolve_executable(opencode_bin).is_none() {
                return Err(corral_core::bin::missing_executable_error(
                    corral_core::harness::Harness::Opencode,
                    opencode_bin,
                ));
            }

            // opencode's TUI attaches to a separate headless HTTP server
            // rather than acting as a self-contained PTY program the way
            // `claude` does, so a session here means two child processes:
            // this `serve` process (never attached to the PTY — its output
            // goes nowhere corral cares about) and the `attach` process
            // spawned into the PTY below. Both are tracked on `Session` and
            // killed together (see `Session::kill`).
            let port = corral_core::opencode::free_port().str_err()?;
            let base_url = corral_core::opencode::base_url(port);
            opencode_base_url = Some(base_url.clone());

            let mut server_cmd = std::process::Command::new(opencode_bin);
            server_cmd
                .args([
                    "serve",
                    "--port",
                    &port.to_string(),
                    "--hostname",
                    "127.0.0.1",
                ])
                .current_dir(&path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let server_child = server_cmd.spawn().str_err()?;
            // `portable_pty` 0.9.0 implements `Child`/`ChildKiller` directly
            // for `std::process::Child` (unix `kill()` sends SIGHUP first,
            // then a real kill after a grace period) — no wrapper type is
            // needed or exists for this. Handed to the drop-guard
            // immediately so a `wait_until_ready`/`create_session` failure
            // below can't leak it.
            opencode_server_guard.0 = Some(Box::new(server_child));
            corral_core::opencode::wait_until_ready(&base_url, std::time::Duration::from_secs(10))
                .str_err()?;

            let opencode_stored = stored
                .as_ref()
                .filter(|s| s.harness == corral_core::harness::Harness::Opencode);
            let session_id = match opencode_stored {
                Some(s) => {
                    resumed = true;
                    s.session_id.clone()
                }
                None => {
                    resumed = false;
                    let new_id = corral_core::opencode::create_session(&base_url).str_err()?;
                    corral_core::agent_session::set(
                        &id,
                        corral_core::harness::Harness::Opencode,
                        &new_id,
                    )
                    .str_err()?;
                    // Same onboarding briefing the Claude arm appends via
                    // `--append-system-prompt` — see `opencode::send_context`
                    // and `onboarding.rs`'s module doc comment for why this
                    // needs to be a real (silent) message instead of
                    // opencode's per-request-only `system` field, and why it
                    // must happen now, before `attach` is spawned below and
                    // starts making its own HTTP calls.
                    let prompt = corral_core::onboarding::system_prompt(
                        &project,
                        &feature,
                        &existing_title,
                        &settings.extra_instructions,
                        settings.co_author_trailer,
                        &project_settings.project_prompt,
                    );
                    corral_core::opencode::send_context(&base_url, &new_id, &prompt).str_err()?;
                    new_id
                }
            };
            opencode_session_id = Some(session_id.clone());

            let mut cmd = CommandBuilder::new(opencode_bin);
            cmd.args(["attach", &base_url, "--session", &session_id, "--dir"]);
            cmd.arg(&path);
            pty_cmd = Some(cmd);
        }
        corral_core::harness::Harness::Opencode2 => {
            let opencode2_bin = if settings.opencode2_executable_path.trim().is_empty() {
                "opencode"
            } else {
                settings.opencode2_executable_path.trim()
            };
            if corral_core::bin::resolve_executable(opencode2_bin).is_none() {
                return Err(corral_core::bin::missing_executable_error(
                    corral_core::harness::Harness::Opencode2,
                    opencode2_bin,
                ));
            }

            // Run a dedicated TCP server so Corral can observe V2's native
            // session.execution lifecycle events without scraping the TUI.
            // The server remains per-session and is killed with the PTY child.
            let port = corral_core::opencode::free_port().str_err()?;
            let base_url = corral_core::opencode::base_url(port);
            let password = new_opencode2_token(&id);
            let mut server_cmd = std::process::Command::new(opencode2_bin);
            server_cmd
                .args([
                    "serve",
                    "--hostname",
                    "127.0.0.1",
                    "--port",
                    &port.to_string(),
                ])
                .current_dir(&path)
                .env("OPENCODE_PASSWORD", &password)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let server_child = server_cmd.spawn().str_err()?;
            opencode_server_guard.0 = Some(Box::new(server_child));
            corral_core::opencode::wait_until_ready_with_password(
                &base_url,
                &password,
                std::time::Duration::from_secs(10),
            )
            .str_err()?;
            opencode_base_url = Some(base_url.clone());
            opencode_password = Some(password.clone());

            // Resume the worktree's last opencode2 session unless it's gone
            // from the server (e.g. the opencode2 store was cleared
            // out-of-band), in which case start fresh — mirroring the Claude
            // and legacy-opencode arms. A fresh session is created up front
            // (before the TUI starts) so Corral knows its id and can inject
            // the onboarding briefing as a real session instruction entry —
            // V2's analog of claude's `--append-system-prompt`, privileged
            // always-in-scope context rather than a first chat turn. The TUI
            // is then pointed at that id via `--session` so it attaches to
            // the seeded session instead of creating a fresh, unbriefed one.
            let opencode2_stored = stored
                .as_ref()
                .filter(|s| s.harness == corral_core::harness::Harness::Opencode2);
            let session_id = match opencode2_stored {
                Some(s)
                    if corral_core::opencode::session_exists_v2(
                        &base_url,
                        &password,
                        &s.session_id,
                    )
                    .str_err()? =>
                {
                    resumed = true;
                    s.session_id.clone()
                }
                _ => {
                    resumed = false;
                    let new_id =
                        corral_core::opencode::create_session_v2(&base_url, &password, &path)
                            .str_err()?;
                    // Record the session id immediately, before the TUI is
                    // spawned, so this worktree is resumable even if corral
                    // dies mid-spawn (the Claude/Opencode arms do the same).
                    corral_core::agent_session::set(
                        &id,
                        corral_core::harness::Harness::Opencode2,
                        &new_id,
                    )
                    .str_err()?;
                    let prompt = corral_core::onboarding::system_prompt(
                        &project,
                        &feature,
                        &existing_title,
                        &settings.extra_instructions,
                        settings.co_author_trailer,
                        &project_settings.project_prompt,
                    );
                    corral_core::opencode::put_instruction_entry(
                        &base_url,
                        &password,
                        &new_id,
                        "corral-onboarding",
                        &prompt,
                    )
                    .str_err()?;
                    new_id
                }
            };

            // Same root id the legacy arm records above: the event stream
            // needs it to distinguish this session's turn from its subagent
            // child sessions.
            opencode_session_id = Some(session_id.clone());

            // The browser tools are corral-specific (embedded WebKit window,
            // unsupported Chromium-only operations), so keep a dedicated
            // instruction entry in scope for resumed sessions too — their
            // original onboarding entry may predate this feature. Best-effort:
            // a failure only means the model misses the extra guidance.
            if let Err(e) = corral_core::opencode::put_instruction_entry(
                &base_url,
                &password,
                &session_id,
                "corral-browser",
                corral_core::onboarding::BROWSER_INSTRUCTIONS,
            ) {
                tracing::warn!("start_session: failed to add browser instructions: {e}");
            }

            // opencode restores a persistent tab strip per working directory.
            // Drop this worktree's entry before the TUI starts so tabs left
            // over from earlier sessions (including ones corral already
            // deleted) don't reappear next to the session we're attaching.
            // The TUI adds the launched session back as the only tab.
            corral_core::opencode::forget_tabs_for_directory(&path);

            let mut cmd = CommandBuilder::new(opencode2_bin);
            cmd.args(["--server", &base_url, "--session", &session_id]);
            // opencode2's analog of claude's `--dangerously-skip-permissions`:
            // auto-approves permissions except ones the config explicitly
            // denies. Wired to the same `skip_permissions` setting.
            if settings.skip_permissions {
                cmd.arg("--auto");
            }
            cmd.arg(&path);
            opencode2_token = Some(new_opencode2_token(&id));
            pty_cmd = Some(cmd);
        }
        corral_core::harness::Harness::Pi => {
            let pi_bin = if settings.pi_executable_path.trim().is_empty() {
                "pi"
            } else {
                settings.pi_executable_path.trim()
            };
            if corral_core::bin::resolve_executable(pi_bin).is_none() {
                return Err(corral_core::bin::missing_executable_error(
                    corral_core::harness::Harness::Pi,
                    pi_bin,
                ));
            }

            // Install corral's status extension and point pi at it. A failure
            // here (e.g. an unwritable data dir) is non-fatal: the session
            // still runs, it just falls back to no real waiting/working signal.
            let status_extension = match corral_core::pi::ensure_status_extension() {
                Ok(path) => Some(path),
                Err(e) => {
                    tracing::warn!("start_session: failed to install pi status extension: {e}");
                    None
                }
            };

            let pi_stored = stored
                .as_ref()
                .filter(|s| s.harness == corral_core::harness::Harness::Pi);
            let mut cmd = CommandBuilder::new(pi_bin);
            match pi_stored {
                Some(s) => {
                    resumed = true;
                    // `--session-id` opens the exact project session, or
                    // creates it if absent — so unlike Claude there's no
                    // "conversation not found" failure to recover from.
                    cmd.args(["--session-id", &s.session_id]);
                }
                None => {
                    resumed = false;
                    let pi_id = corral_core::agent_session::new_id();
                    corral_core::agent_session::set(&id, corral_core::harness::Harness::Pi, &pi_id)
                        .str_err()?;
                    cmd.args(["--session-id", &pi_id]);
                    let prompt = corral_core::onboarding::system_prompt(
                        &project,
                        &feature,
                        &existing_title,
                        &settings.extra_instructions,
                        settings.co_author_trailer,
                        &project_settings.project_prompt,
                    );
                    cmd.args(["--append-system-prompt", &prompt]);
                }
            }
            // Pi trusts project-local config/resources only after a prompt the
            // TUI would otherwise block on; corral worktrees are the user's own
            // checkouts, so trust them up front. Pi has no tool-permission
            // system, so `settings.skip_permissions` has nothing to map to.
            cmd.arg("--approve");
            if let Some(path) = &status_extension {
                cmd.arg("--extension");
                cmd.arg(path);
            }
            pty_cmd = Some(cmd);
        }
    };

    if let Some(cmd) = &mut pty_cmd {
        cmd.cwd(&path);
        // Project-configured env vars (API keys, feature flags) — applied
        // before the internal vars below so the hook socket / session id /
        // TERM / marker / password and the denylist strip all take precedence
        // over them.
        for (key, value) in project_settings.env_vars() {
            cmd.env(key, value);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env(
            corral_core::hooks::HOOK_SOCKET_ENV,
            corral_core::hooks::socket_path(),
        );
        // The registry key for this session, so harness-side status reporters
        // that talk to the hook socket directly (the pi extension — see
        // `corral_core::pi`) can identify themselves without resolving a
        // worktree from `cwd`.
        cmd.env("CORRAL_SESSION_ID", &id);
        // Only for `Harness::Opencode2`: tag every process the TUI spawns
        // (including the detached `serve --stdio` it abandons) so
        // `kill_opencode2_children` can find them on session end.
        if let Some(token) = &opencode2_token {
            cmd.env(OPENCODE2_MARKER, token);
        }
        if let Some(password) = &opencode_password {
            cmd.env("OPENCODE_PASSWORD", password);
        }
        // If corral itself was launched from inside a Claude Code session
        // (e.g. dev-testing via a Bash tool), these leak in and make the
        // nested `claude` child skip its normal transcript persistence —
        // breaking --resume. Strip them so the spawned agent always behaves
        // like a normal, top-level session.
        for var in [
            "CLAUDECODE",
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_EXECPATH",
            "CLAUDE_CODE_SSE_PORT",
            "CLAUDE_EFFORT",
            "AI_AGENT",
        ] {
            cmd.env_remove(var);
        }
    }

    let (initial_waiting, initial_waiting_since) = (true, Some(Instant::now()));

    let child = pair
        .slave
        .spawn_command(pty_cmd.expect("all harnesses set pty_cmd"))
        .str_err()?;
    // Drop the slave handle so the reader sees EOF once claude exits.
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().str_err()?;
    let writer = pair.master.take_writer().str_err()?;

    let session = Arc::new(Session {
        id: id.clone(),
        project,
        feature,
        master: Mutex::new(pair.master),
        writer: Mutex::new(writer),
        child: Mutex::new(child),
        // `take()` here (rather than reading `opencode_server_guard.0`
        // directly) hands ownership to the `Session` and leaves the guard
        // holding `None`, so its `Drop` — which runs when `start_session`
        // returns, success or not — becomes a no-op instead of killing the
        // process we just successfully handed off.
        opencode_server: opencode_server_guard.take().map(Mutex::new),
        opencode2_token,
        inner: Mutex::new(Inner {
            buffer: Vec::new(),
            running: true,
            exit_code: None,
            killed: false,
            exited_at: None,
            waiting: initial_waiting,
            waiting_since: initial_waiting_since,
            last_summary: None,
        }),
        max_buffer: scrollback_bytes(settings.scrollback_kb),
        resumed,
    });

    registry.sessions.lock().insert(id.clone(), session.clone());
    sync_caffeinate(&app);

    tracing::info!(
        "start_session: spawned {:?} for {}/{} (resume={})",
        settings.harness,
        session.project,
        session.feature,
        resumed
    );
    if let Some(base_url) = opencode_base_url {
        match opencode_session_id {
            Some(root_session_id) => {
                crate::opencode_events::spawn(
                    app.clone(),
                    id.clone(),
                    root_session_id.clone(),
                    base_url.clone(),
                    opencode_password.clone(),
                );
                // OpenCode's built-in `browser.*` tools (including
                // `browser.preview`) only exist while a desktop client is
                // attached. Only V2 has that protocol — V1 sessions have no
                // password and are skipped.
                if let Some(password) = opencode_password {
                    crate::browser_host::spawn(
                        app.clone(),
                        id.clone(),
                        session.project.clone(),
                        session.feature.clone(),
                        path.clone(),
                        crate::browser_host::OpenCodeSession {
                            root_session_id,
                            base_url,
                            password,
                        },
                    );
                }
            }
            None => tracing::warn!(
                "start_session: opencode server at {base_url} has no root session id; status updates disabled"
            ),
        }
    }
    spawn_reader(app, session.clone(), reader);
    Ok(session.info())
}

/// Pump PTY output into the scrollback buffer and out as events, until EOF.
fn spawn_reader(app: AppHandle, session: Arc<Session>, mut reader: Box<dyn Read + Send>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Err(e) => {
                    tracing::warn!(
                        "session {}/{}: PTY read error, ending session: {e}",
                        session.project,
                        session.feature
                    );
                    break;
                }
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    {
                        let mut inner = session.inner.lock();
                        super::append_capped(&mut inner.buffer, &chunk, session.max_buffer);
                    }
                    let _ = app.emit(
                        "session-output",
                        OutputEvent {
                            id: session.id.clone(),
                            data: chunk,
                        },
                    );
                }
            }
        }

        let code = match session.child.lock().wait() {
            Ok(status) => Some(status.exit_code().try_into().unwrap_or(i32::MAX)),
            Err(e) => {
                tracing::warn!(
                    "session {}/{}: failed to wait on child process: {e}",
                    session.project,
                    session.feature
                );
                None
            }
        };
        // The primary `child` (`opencode attach`, or the opencode TUI) just
        // exited on its own — most commonly the user
        // quit the TUI, or it crashed/lost its connection to the paired
        // server. `Session::kill()` never runs on this path (nobody called
        // Stop), so without this the paired `opencode serve` (and, for
        // Opencode2, the detached `serve --stdio`) would keep running until
        // the session is later explicitly killed or the `Session` itself is
        // dropped — this is the most common real-world way a session's life
        // ends, so it's worth killing the server promptly here rather than
        // relying only on `Session::kill()`/`Drop` to eventually catch it.
        session.kill_opencode_server();
        session.sweep_opencode2_markers();
        let (killed, resume_failed) = {
            let mut inner = session.inner.lock();
            inner.running = false;
            inner.exit_code = code;
            inner.exited_at = Some(Instant::now());
            let resume_failed = session.resumed
                && !inner.killed
                && find_subslice(&inner.buffer, RESUME_FAILURE_MARKER);
            (inner.killed, resume_failed)
        };
        sync_caffeinate(&app);
        tracing::info!(
            "session {}/{} exited (code={:?}, killed={}, resume_failed={})",
            session.project,
            session.feature,
            code,
            killed,
            resume_failed
        );
        let _ = app.emit(
            "session-exit",
            ExitEvent {
                id: session.id.clone(),
                code,
                killed,
                resume_failed,
            },
        );
    });
}

#[tauri::command]
pub fn attach_session(registry: State<SessionRegistry>, id: String) -> Result<Attach, String> {
    let sessions = registry.sessions.lock();
    let s = sessions.get(&id).ok_or("session not found")?;
    let info = s.info();
    let buffer = s.inner.lock().buffer.clone();
    Ok(Attach { info, buffer })
}

#[tauri::command]
pub fn write_session(
    _app: AppHandle,
    registry: State<SessionRegistry>,
    id: String,
    data: String,
) -> Result<(), String> {
    let sessions = registry.sessions.lock();
    let s = sessions.get(&id).ok_or("session not found")?;
    let mut writer = s.writer.lock();
    writer.write_all(data.as_bytes()).str_err()?;
    writer.flush().str_err()?;
    Ok(())
}

#[tauri::command]
pub fn resize_session(
    registry: State<SessionRegistry>,
    id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let sessions = registry.sessions.lock();
    let s = sessions.get(&id).ok_or("session not found")?;
    s.master
        .lock()
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .str_err()?;
    Ok(())
}

#[tauri::command]
pub fn kill_session(registry: State<SessionRegistry>, id: String) -> Result<(), String> {
    let sessions = registry.sessions.lock();
    let s = sessions.get(&id).ok_or("session not found")?;
    s.kill().str_err()
}

/// Best-effort kill of a session by id *and* its removal from the registry,
/// silently doing nothing if none is registered — used by `remove_worktree`
/// and `remove_project` (`commands/projects.rs`), both of which are deleting
/// the worktree(s), so the entry must not linger. A later worktree reusing the
/// same `project/feature` would otherwise find a stale `Arc<Session>` (though
/// `start_session` would replace it). Unlike the `kill_session` command, a
/// missing session isn't an error here: most of a project's worktrees won't
/// have a live session at all.
///
/// The registry lock is released before `kill()` runs, so this can't hold it
/// across the child-process wait/probe.
pub(crate) fn kill_session_best_effort(registry: &SessionRegistry, id: &str) {
    let session = registry.sessions.lock().remove(id);
    if let Some(s) = session
        && let Err(e) = s.kill()
    {
        tracing::warn!("kill_session_best_effort: failed to kill session {id}: {e}");
    }
}

/// Drop an exited session from the registry (e.g. after the user dismisses it).
#[tauri::command]
pub fn close_session(registry: State<SessionRegistry>, id: String) -> Result<(), String> {
    registry.sessions.lock().remove(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spawns a real PTY-backed `Session` running `sh -c command`, so
    /// `kill()`'s `try_wait()` probe exercises a genuine child process
    /// instead of a mock. `opencode_server` is always `None` — use
    /// [`spawn_test_session_with_server`] for tests that need a paired
    /// server process too.
    ///
    /// Built via a plain function (not `Session { ..other }` update syntax)
    /// deliberately: `Session` implements `Drop`, and Rust disallows partial
    /// moves out of a `Drop` type, so `..other` can't be used to build one
    /// `Session` from another's fields at all — both this and
    /// `spawn_test_session_with_server` construct their `Session` fully
    /// inline instead.
    fn spawn_test_session(command: &str) -> Session {
        spawn_test_session_with_opencode_server(command, None)
    }

    /// Returns a `Session` whose `child` is a real `sh -c command` PTY
    /// process and whose `opencode_server` is a real, running `sh -c
    /// server_command` process — used by the `opencode_server` kill tests
    /// below.
    fn spawn_test_session_with_server(server_command: &str) -> Session {
        spawn_test_session_with_opencode_server("exit 0", Some(server_command))
    }

    fn spawn_test_session_with_opencode_server(
        command: &str,
        server_command: Option<&str>,
    ) -> Session {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.arg("-c");
        cmd.arg(command);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let opencode_server = server_command.map(|server_command| {
            Mutex::new(Box::new(
                std::process::Command::new("sh")
                    .arg("-c")
                    .arg(server_command)
                    .spawn()
                    .unwrap(),
            ) as Box<dyn Child + Send + Sync>)
        });
        Session {
            id: "proj/feat".into(),
            project: "proj".into(),
            feature: "feat".into(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            opencode_server,
            opencode2_token: None,
            inner: Mutex::new(Inner {
                running: true,
                ..Default::default()
            }),
            max_buffer: 1024,
            resumed: true,
        }
    }

    /// The core regression this guards against: an opencode session pairs a
    /// headless `opencode serve` process with the visible PTY `child`
    /// (`opencode attach`) — `kill()` must terminate both, not just the PTY
    /// child, or a stopped session would leave `opencode serve` running
    /// forever with nothing left in the UI to show or stop it.
    #[test]
    fn kill_terminates_the_paired_opencode_server_too() {
        let session = spawn_test_session_with_server("sleep 30");

        session.kill().unwrap();

        let server = session.opencode_server.as_ref().unwrap();
        for _ in 0..200 {
            if matches!(server.lock().try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("opencode_server was not killed alongside the session");
    }

    /// A Claude session has no paired server process — `kill()` must not
    /// panic or error just because `opencode_server` is `None`.
    #[test]
    fn kill_is_fine_with_no_opencode_server() {
        let session = spawn_test_session("sleep 30");
        assert!(session.opencode_server.is_none());
        session.kill().unwrap();
    }

    /// The core regression a code-review round found in the first version of
    /// this fix: `kill()`'s already-exited early return (for the primary
    /// `child`, i.e. `opencode attach`) used to happen *before*
    /// `opencode_server` was ever touched — so if `attach` had already
    /// exited on its own (crash, the user quitting the TUI, losing its
    /// connection to `serve`, ...) by the time anything called `kill()`, the
    /// paired `opencode serve` was never killed by this method at all. This
    /// is a materially more common trigger than the primary-child-still-
    /// running case `kill_terminates_the_paired_opencode_server_too` covers.
    #[test]
    fn kill_terminates_opencode_server_even_when_primary_child_already_exited() {
        let session = spawn_test_session_with_server("sleep 30");

        // Let the primary (placeholder) `child` finish on its own first, so
        // `kill()`'s `try_wait()` probe finds it already exited.
        for _ in 0..200 {
            if matches!(session.child.lock().try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(session.child.lock().try_wait(), Ok(Some(_))));

        session.kill().unwrap();

        let server = session.opencode_server.as_ref().unwrap();
        for _ in 0..200 {
            if matches!(server.lock().try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("opencode_server was not killed when the primary child had already exited");
    }

    /// Backstop coverage for `Session`'s `Drop` impl: if a `Session` is
    /// dropped without `kill()` or `spawn_reader`'s natural-exit handling
    /// having run first (e.g. `close_session` racing ahead of the reader
    /// thread's own cleanup), the paired `opencode_server` must still get
    /// killed rather than silently leaking — dropping a bare
    /// `std::process::Child` does not kill it.
    #[test]
    fn dropping_a_session_kills_its_opencode_server() {
        let session = spawn_test_session_with_server("sleep 30");
        let server_pid = session
            .opencode_server
            .as_ref()
            .unwrap()
            .lock()
            .process_id()
            .unwrap();

        drop(session);

        for _ in 0..200 {
            if !process_alive(server_pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("opencode_server was not killed when the Session was dropped");
    }

    /// The regression `KillServerOnDrop` exists to prevent: `start_session`'s
    /// opencode arm spawns `opencode serve` before several more fallible
    /// steps (`wait_until_ready`, `create_session`, the final PTY spawn).
    /// Dropping a bare `std::process::Child` does *not* kill the process —
    /// so if the guard is dropped while still holding a child (simulating an
    /// early `?` return on one of those later steps), the process must
    /// actually die; if `take()` was called first (simulating the success
    /// path, where ownership moves into the constructed `Session`), the
    /// guard's drop must be a no-op and leave the process running.
    /// Whether `pid` still refers to a live process — shells out to `kill
    /// -0` rather than pulling in a `libc` dependency just for this test,
    /// since by this point in the test we've deliberately given up our
    /// `Child` handle (simulating the guard's `Drop` having run) and so have
    /// no `try_wait()` of our own left to probe with.
    fn process_alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn kill_server_on_drop_kills_an_untaken_child() {
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .spawn()
            .unwrap();
        let pid = child.id();
        {
            let _guard = KillServerOnDrop(Some(Box::new(child)));
            // Guard drops here, simulating an early `?` return.
        }
        for _ in 0..200 {
            if !process_alive(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("KillServerOnDrop did not kill the child it was still holding");
    }

    #[test]
    fn kill_server_on_drop_leaves_a_taken_child_running() {
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut taken = {
            let mut guard = KillServerOnDrop(Some(Box::new(child)));
            let taken = guard.take();
            assert!(taken.is_some(), "take() should hand back the wrapped child");
            taken.unwrap()
            // Guard drops here holding `None` — simulating the success path,
            // where the taken child was handed off to the `Session`.
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            process_alive(pid),
            "child should still be alive — take() should have defused the guard"
        );
        let _ = taken.kill();
        let _ = taken.wait();
    }

    /// The core regression this guards against: if a session's process
    /// already exited on its own (e.g. crashed on `--resume` failure)
    /// between the idle sweep's `running` check and its `kill()` call,
    /// `kill()` must not stamp `killed = true` — doing so would make the
    /// reader thread's `resume_failed` check (`!inner.killed`) evaluate
    /// `false`, masking a real resume failure as a deliberate stop.
    #[test]
    fn kill_does_not_mark_killed_when_process_already_exited() {
        let session = spawn_test_session("exit 0");
        // Give the process a moment to actually terminate before probing —
        // `try_wait()` only reports what's already reaped.
        for _ in 0..200 {
            if matches!(session.child.lock().try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        session.kill().unwrap();
        assert!(!session.inner.lock().killed);
    }

    #[test]
    fn kill_marks_killed_and_terminates_a_still_running_process() {
        let session = spawn_test_session("sleep 30");
        session.kill().unwrap();
        assert!(session.inner.lock().killed);
        // The kill should actually take effect soon after.
        for _ in 0..200 {
            if matches!(session.child.lock().try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("process did not exit after kill()");
    }

    #[test]
    fn kill_session_best_effort_kills_a_registered_session() {
        let registry = SessionRegistry::default();
        let session = Arc::new(spawn_test_session("sleep 30"));
        registry
            .sessions
            .lock()
            .insert("proj/feat".to_string(), session.clone());

        kill_session_best_effort(&registry, "proj/feat");

        assert!(session.inner.lock().killed);
        assert!(
            !registry.sessions.lock().contains_key("proj/feat"),
            "the removed session's registry entry must not linger"
        );
    }

    #[test]
    fn kill_session_best_effort_is_a_noop_for_an_unregistered_id() {
        let registry = SessionRegistry::default();
        // Must not panic just because there's nothing to kill.
        kill_session_best_effort(&registry, "no/such-session");
    }

    #[test]
    fn still_running_session_is_never_stale() {
        assert!(!is_stale_exited(None, Duration::ZERO));
    }

    #[test]
    fn recently_exited_session_is_not_yet_stale() {
        assert!(!is_stale_exited(
            Some(Instant::now()),
            Duration::from_secs(3600)
        ));
    }

    #[test]
    fn exited_session_past_retention_is_stale() {
        let exited_at = Instant::now() - Duration::from_secs(1000);
        assert!(is_stale_exited(Some(exited_at), Duration::from_secs(10)));
    }

    #[test]
    fn exited_session_within_retention_is_not_stale() {
        let exited_at = Instant::now() - Duration::from_secs(5);
        assert!(!is_stale_exited(Some(exited_at), Duration::from_secs(3600)));
    }

    #[test]
    fn find_subslice_matches_anywhere_in_haystack() {
        assert!(find_subslice(
            b"garbled\r\nNo conversation found with session ID: abc\r\n",
            RESUME_FAILURE_MARKER
        ));
    }

    #[test]
    fn find_subslice_no_match_returns_false() {
        assert!(!find_subslice(
            b"totally normal output",
            RESUME_FAILURE_MARKER
        ));
    }

    #[test]
    fn find_subslice_empty_haystack_returns_false() {
        assert!(!find_subslice(b"", RESUME_FAILURE_MARKER));
    }

    #[test]
    fn scrollback_bytes_clamps_below_minimum() {
        assert_eq!(scrollback_bytes(1), 64 * 1024);
    }

    #[test]
    fn scrollback_bytes_passes_through_in_range_value() {
        assert_eq!(scrollback_bytes(256), 256 * 1024);
    }

    #[test]
    fn scrollback_bytes_clamps_above_maximum() {
        assert_eq!(scrollback_bytes(999_999), 2048 * 1024);
    }

    /// Mirrors the reservation `start_session` takes across its "already
    /// running?" check and the slow PTY spawn: a second attempt to reserve
    /// the same id while the first is still in flight must fail, and the id
    /// must become reservable again once the first guard drops (spawn done,
    /// success or error).
    #[test]
    fn starting_reservation_rejects_concurrent_id_until_released() {
        let registry = SessionRegistry::default();
        let id = "proj/feat".to_string();

        assert!(registry.starting.lock().insert(id.clone()));
        let guard = StartGuard {
            registry: &registry,
            id: id.clone(),
        };

        // A second concurrent start for the same id must be rejected while
        // the first is still mid-spawn.
        assert!(!registry.starting.lock().insert(id.clone()));

        drop(guard);

        // Released once the first attempt's guard drops, so a retry (or the
        // now-registered session's own next start) can reserve it again.
        assert!(registry.starting.lock().insert(id));
    }

    #[test]
    fn next_waiting_since_starts_a_clock_on_false_to_true_transition() {
        assert!(next_waiting_since(None, false, true).is_some());
    }

    #[test]
    fn next_waiting_since_keeps_existing_clock_on_true_to_true_transition() {
        let existing = Instant::now();
        assert_eq!(
            next_waiting_since(Some(existing), true, true),
            Some(existing)
        );
    }

    #[test]
    fn next_waiting_since_clears_on_transition_to_false() {
        assert_eq!(next_waiting_since(Some(Instant::now()), true, false), None);
    }

    #[test]
    fn next_waiting_since_stays_none_while_never_waiting() {
        assert_eq!(next_waiting_since(None, false, false), None);
    }

    #[test]
    fn should_idle_stop_is_false_when_not_waiting() {
        assert!(!should_idle_stop(true, None, Duration::ZERO));
    }

    #[test]
    fn should_idle_stop_is_false_before_the_timeout_elapses() {
        assert!(!should_idle_stop(
            true,
            Some(Instant::now()),
            Duration::from_secs(3600)
        ));
    }

    #[test]
    fn should_idle_stop_is_true_once_the_timeout_elapses() {
        let waiting_since = Instant::now() - Duration::from_secs(1000);
        assert!(should_idle_stop(
            true,
            Some(waiting_since),
            Duration::from_secs(10)
        ));
    }

    /// The core regression this guards against: a session whose process
    /// already exited (crash or explicit stop) keeps whatever `waiting_since`
    /// it had at the moment it exited — the reader thread clears `running`
    /// but never touches `waiting_since`. Without the `running` check, a
    /// later idle-sweep tick would match this stale `waiting_since` and call
    /// `kill()` again on an already-dead session, flipping `killed` to `true`
    /// post-exit and making a crash look like a deliberate stop.
    #[test]
    fn should_idle_stop_is_false_once_not_running_even_if_timeout_elapsed() {
        let waiting_since = Instant::now() - Duration::from_secs(1000);
        assert!(!should_idle_stop(
            false,
            Some(waiting_since),
            Duration::from_secs(10)
        ));
    }

    /// The core regression the opencode2 marker sweep guards against: a
    /// process carrying a session's `CORRAL_OC2_TOKEN` marker must die when
    /// `kill_opencode2_children` runs, no matter how it's parented (the real
    /// `serve --stdio` is detached/re-parented, so the sweep has to work by
    /// marker, not by process-tree ancestry).
    #[test]
    fn kill_opencode2_children_kills_marked_processes() {
        let token = new_opencode2_token("proj/feat");
        // Spawn `sleep` directly rather than `sh -c "sleep 30"`: a shell
        // re-execs (dash replaces itself with the single command), and reading
        // /proc/<pid>/environ during that second exec can transiently come
        // back empty — the sweep would skip the process and it'd survive,
        // flaking the test under load.
        let child = std::process::Command::new("sleep")
            .arg("30")
            .env(OPENCODE2_MARKER, &token)
            .spawn()
            .unwrap();
        let pid = child.id();
        // The real `Child` handle is deliberately dropped, like the real
        // `serve --stdio`: the sweep finds it by marker, not by handle.
        drop(child);

        // Even a direct spawn can briefly race the sweep's /proc environ read
        // on a loaded CI runner, so wait until the marker is actually
        // observable before sweeping — otherwise the sweep skips the process
        // and it survives.
        wait_for_marker_visible(pid, &token);

        kill_opencode2_children(&token);

        for _ in 0..200 {
            if !process_running(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("marked process survived kill_opencode2_children");
    }

    /// The sweep must only kill processes bearing the session's own marker —
    /// a different corral session's processes (and the user's own opencode
    /// processes, which never carry a marker) must be left untouched.
    #[test]
    fn kill_opencode2_children_spares_unmarked_processes() {
        let token = new_opencode2_token("proj/feat");
        let other_token = new_opencode2_token("proj/other");
        let unmarked = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let other_marked = std::process::Command::new("sleep")
            .arg("30")
            .env(OPENCODE2_MARKER, &other_token)
            .spawn()
            .unwrap();
        let unmarked_pid = unmarked.id();
        let other_marked_pid = other_marked.id();
        drop(unmarked);
        drop(other_marked);

        kill_opencode2_children(&token);

        std::thread::sleep(Duration::from_millis(50));
        assert!(
            process_running(unmarked_pid),
            "sweep must not kill a process with no marker"
        );
        assert!(
            process_running(other_marked_pid),
            "sweep must not kill a process bearing a different session's marker"
        );
    }

    /// Whether `pid` is a live, running process (not a zombie). The existing
    /// `process_alive` helper (kill -0) reports zombies as alive, which is
    /// fine for its callers — the guard-based tests reap their children via
    /// portable_pty's kill (which polls `try_wait`). The marker sweep uses
    /// raw `kill -9` with no reaping, so its tests must treat a zombie
    /// (killed but unreaped) as dead.
    fn process_running(pid: u32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        // State is the third whitespace-separated field (pid, comm, state).
        let Some(state) = stat.split_whitespace().nth(2) else {
            return false;
        };
        state != "Z"
    }

    /// Poll until `pid`'s `/proc/<pid>/environ` actually contains the marker.
    /// `kill_opencode2_children` finds processes by reading that environ, and
    /// on a loaded CI runner even a directly-spawned `sleep` can briefly read
    /// back empty environ mid-exec — which would make the sweep skip it. Wait
    /// for the marker to be observable first so the sweep is deterministic.
    fn wait_for_marker_visible(pid: u32, token: &str) {
        let needle = format!("{OPENCODE2_MARKER}={token}");
        for _ in 0..200 {
            if let Ok(environ) = std::fs::read(format!("/proc/{pid}/environ"))
                && environ.split(|b| *b == 0).any(|kv| kv == needle.as_bytes())
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("marker never became visible in /proc/{pid}/environ");
    }

    #[test]
    fn new_opencode2_token_is_unique_per_call() {
        let a = new_opencode2_token("proj/feat");
        let b = new_opencode2_token("proj/feat");
        assert_ne!(a, b);
        assert!(a.contains("proj/feat"));
    }
}
