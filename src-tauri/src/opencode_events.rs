//! Background thread that subscribes to an opencode server's Server-Sent
//! Events stream and forwards session status transitions into
//! `SessionRegistry`, the same sink Claude Code's Unix-socket hook listener
//! (`hook_listener.rs`) feeds. opencode has no hook system, so SSE is its
//! equivalent status source.
//!
//! # Event names and endpoint: what's confirmed live vs. inferred
//!
//! This module's initial implementation assumed a per-session endpoint
//! `GET /api/session/{id}/event`
//! framed as `event: <name>\ndata: <json>`, with event names guessed as
//! `session.idle` / `permission.asked` / `permission.replied`. Live
//! verification against opencode 1.18.13 (installed on this dev machine,
//! `opencode --version`) on 2026-08-07 found that assumption **wrong** in
//! two ways, corrected below:
//!
//! 1. **Endpoint.** `GET /api/session/{id}/event` does not exist in this
//!    opencode version — the connection hangs with no HTTP response at all
//!    (confirmed with `curl -v`, 8s timeout, zero bytes back, not even
//!    headers). The only event-subscribe route is the *global*
//!    `GET /event` (optionally `?directory=`), confirmed both by a live
//!    `curl` capture (see below) and by the installed SDK's generated
//!    types (`~/.config/opencode/node_modules/@opencode-ai/sdk/dist/gen/types.gen.d.ts`,
//!    `EventSubscribeData { url: "/event" }`, matching the exact installed
//!    binary version). Since each opencode session in corral gets its own
//!    dedicated headless `opencode serve` process (see
//!    `commands::session::start_session`'s Opencode arm), that server's
//!    global stream only carries this corral session's tree — but it *does*
//!    carry child sessions (subagents), so this module tracks the root and
//!    its descendants explicitly rather than assuming every event belongs
//!    to the session corral spawned.
//! 2. **Framing.** There is no separate `event: <name>` line on this
//!    endpoint — only `data: <json>\n\n`, with the event's type living
//!    inside the JSON payload as a top-level `"type"` field, e.g.
//!    `{"id":"evt_...","type":"session.idle","properties":{"sessionID":"..."}}`.
//!    V2 follows the same framing at `GET /api/event`, but puts event payloads
//!    under `data` and reports turns with `session.execution.started` and
//!    `session.execution.succeeded` (confirmed live against beta-18155).
//!
//! Event name evidence, per name used in `StatusTracker::apply` below:
//!
//! - `session.idle` (a turn finished, agent now waiting on the user) —
//!   **CONFIRMED LIVE** on 2026-08-07: ran `opencode serve` locally, opened
//!   `GET /event`, sent a real message via `POST /session/{id}/message`
//!   (using opencode's bundled default model, "opencode/big-pickle"), and
//!   observed exactly one
//!   `{"type":"session.idle","properties":{"sessionID":"<matching id>"}}`
//!   event once the turn completed.
//! - `session.status` with `properties.status.type == "busy"` (a turn just
//!   started, agent now working) — **CONFIRMED LIVE** in the same capture:
//!   fired repeatedly with `{"type":"busy"}` while the turn was in
//!   progress. Used here as the "working" signal, since (unlike Claude's
//!   hook flow) nothing else in this codebase yet tells corral when an
//!   opencode turn starts.
//! - `permission.updated` / `permission.replied` (blocked on a permission
//!   prompt / prompt answered) — **NOT live-captured.** This sandbox's
//!   bundled default model never triggered a real permission-gated tool
//!   call (it answered `run the shell command: ...` with a plain text
//!   "Done." and no visible tool invocation), so no permission event was
//!   ever observed streaming. These two names instead come from **static
//!   evidence**: the installed SDK's generated types (same file as above)
//!   declare
//!   `EventPermissionUpdated { type: "permission.updated", properties: Permission }`
//!   and
//!   `EventPermissionReplied { type: "permission.replied", properties: { sessionID, permissionID, response } }`
//!   for opencode 1.18.13 specifically — note this is `permission.updated`,
//!   **not** `permission.asked` as the task brief speculated. This is
//!   higher-confidence than a guess (it's the real generated API surface
//!   for the exact installed version, not documentation prose that could
//!   be stale or aspirational), but it is still inference from a type
//!   definition, not an observed event during an actual blocked turn.
//!   **This must be re-verified against a live provider-backed permission
//!   prompt before being trusted in production** — e.g. configure a real
//!   provider, add a restrictive `opencode.json` permission rule, ask the
//!   agent to run a shell command, and confirm `permission.updated` fires
//!   when the prompt appears and `permission.replied` fires (with what
//!   `response` value?) once answered.
//!
//! Background work (confirmed live against 2.0.14 on 2026-10-27): a
//! `background: true` subagent is a *child session*. The root's
//! `session.execution.succeeded` fires while that child is still executing
//! (`session.created` announced it with `data.parentID`), so treating every
//! execution event as the root's turn marked corral's session idle in the
//! middle of subagent work. Background shell commands have the same shape:
//! `session.shell.started` / `session.shell.ended` bracket a shell whose
//! `data.shell.metadata.background` is true, and it can outlive the parent
//! turn. `StatusTracker` therefore aggregates the root plus its child
//! sessions and their live shells, and only reports waiting once all of them
//! are idle.
//!
//! Unrecognized event types are silently ignored (`_ => None` in
//! `StatusTracker::apply`), mirroring `hook_listener.rs`'s own defensive
//! handling of unknown hook events — this is an intentional,
//! already-established pattern in this codebase, not a placeholder.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::session::SessionRegistry;
use crate::hook_listener::StatusEvent;

/// The subset of an opencode SSE event's JSON payload this module cares
/// about. Deliberately loose (`properties` stays a raw `Value`) since the
/// full event union has many variants this module ignores entirely.
#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    properties: Option<serde_json::Value>,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// Subscribe to `base_url`'s global SSE event stream and forward status
/// transitions for corral session `id` into `SessionRegistry`, for the
/// lifetime of the stream (i.e. until the paired `opencode serve` process
/// exits and the connection drops). One call per opencode session — see
/// `commands::session::start_session`'s Opencode arms.
///
/// `id` is Corral's registry key. `root_session_id` is opencode's own id for
/// the session Corral created (`ses_...`), used to tell the root turn apart
/// from the child sessions opencode spawns for subagents. Both harnesses pass
/// it: a V2 server is process-scoped, but its stream still carries child
/// sessions, and treating a child's `session.execution.succeeded` as the root
/// going idle is exactly what hides background subagents.
pub fn spawn(
    app: AppHandle,
    id: String,
    root_session_id: String,
    base_url: String,
    password: Option<String>,
) {
    std::thread::spawn(move || {
        // V2 exposes its event API below /api; the legacy server used /event.
        let url = if password.is_some() {
            format!("{base_url}/api/event")
        } else {
            format!("{base_url}/event")
        };
        let mut request = ureq::get(&url);
        request = request.header("Accept", "text/event-stream");
        if let Some(password) = &password {
            request = request.header(
                "Authorization",
                &corral_core::opencode::basic_auth(password),
            );
        }
        let resp = match request.call() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("opencode_events: failed to subscribe for {id}: {e}");
                return;
            }
        };
        tracing::info!("opencode_events: subscribed for {id} at {url}");
        let reader = std::io::BufReader::new(resp.into_body().into_reader());
        let mut tracker = StatusTracker::new(root_session_id);
        // A freshly spawned session starts in `SessionRegistry` as waiting,
        // matching the tracker's initial all-idle state. Seed the dedupe so a
        // redundant first "idle" event doesn't emit a duplicate status.
        let mut last_reported = Some(true);
        let mut read_error = None;
        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(e) => {
                    read_error = Some(e);
                    break;
                }
            };
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<RawEvent>(data) else {
                tracing::debug!("opencode_events: ignoring unparseable event: {data}");
                continue;
            };
            if let Some(waiting) = tracker.apply(&event)
                && last_reported != Some(waiting)
            {
                last_reported = Some(waiting);
                apply(&app, &id, waiting);
            }
        }
        // A stream ending because the paired `opencode serve` process
        // exited (session stopped/killed normally) is expected and fine.
        // A stream ending because of an actual read error is not — it
        // means this session's status silently freezes at whatever it last
        // was, indistinguishable from "genuinely idle", so it's worth a
        // `warn!` rather than the routine `info!` below.
        match read_error {
            Some(e) => tracing::warn!(
                "opencode_events: stream for {id} ended with a read error (status updates have stopped): {e}"
            ),
            None => tracing::info!("opencode_events: stream ended for {id}"),
        }
    });
}

/// Mutable status state for one opencode server's event stream.
///
/// Status is not a property of a single event: opencode V2 can end the root
/// turn while a background subagent (a child session) or a background shell
/// keeps running. The session stays "working" until every tracked execution
/// and shell in the root's session tree has finished.
struct StatusTracker {
    root: String,
    /// Every known session in this tree, including the root. Child sessions
    /// are added when `session.created` reports their `parentID`; any event
    /// for an unknown session is ignored rather than attributed to the root.
    sessions: HashMap<String, SessionWork>,
    /// Shell id -> owning session id, so `shell.exited` (which carries no
    /// `sessionID`) can still clear the right shell.
    shell_owners: HashMap<String, String>,
    /// Sessions blocked on a permission prompt. An empty set plus no running
    /// work is the only state that means "waiting for the user".
    pending_permissions: HashSet<String>,
}

#[derive(Default)]
struct SessionWork {
    /// A prompt-driven run is in progress for this session.
    executing: bool,
    /// Shell tool calls that started but haven't ended. A background shell
    /// outlives its parent turn, which is why this is tracked separately
    /// from `executing`.
    shells: HashSet<String>,
}

impl StatusTracker {
    fn new(root: String) -> Self {
        let mut sessions = HashMap::new();
        sessions.insert(root.clone(), SessionWork::default());
        Self {
            root,
            sessions,
            shell_owners: HashMap::new(),
            pending_permissions: HashSet::new(),
        }
    }

    /// The state corral should display: waiting for the user only when no
    /// session in the tree is executing, no shell is still running, and no
    /// permission prompt is blocking.
    fn is_waiting(&self) -> bool {
        if !self.pending_permissions.is_empty() {
            return true;
        }
        !self
            .sessions
            .values()
            .any(|work| work.executing || !work.shells.is_empty())
    }

    fn set_executing(&mut self, session_id: Option<&str>, executing: bool) {
        let Some(session_id) = session_id else {
            return;
        };
        let Some(work) = self.sessions.get_mut(session_id) else {
            return;
        };
        work.executing = executing;
        if !executing {
            // A turn ending also resolves any prompt it was blocked on. A
            // pending prompt on a *different* session stays in the set, and
            // therefore keeps the aggregate status waiting.
            self.pending_permissions.remove(session_id);
        }
    }

    /// Consume one SSE event. `Some(waiting)` means the event carried a
    /// status signal (callers dedupe the resulting boolean); `None` means the
    /// event is irrelevant to this tree.
    fn apply(&mut self, event: &RawEvent) -> Option<bool> {
        let data = event
            .properties
            .as_ref()
            .or(event.data.as_ref())
            .unwrap_or(&serde_json::Value::Null);
        let session_id = data.get("sessionID").and_then(|v| v.as_str());

        // Events carrying a session id only affect this tree; `session.created`
        // is the one event that adds a session to it, and the top-level shell
        // events are keyed by shell id instead.
        if !matches!(
            event.kind.as_str(),
            "session.created" | "shell.exited" | "shell.deleted"
        ) && let Some(id) = session_id
            && !self.sessions.contains_key(id)
        {
            return None;
        }

        match event.kind.as_str() {
            // V2 child sessions (subagents) announce their parent here. Only
            // sessions reachable from the root are tracked; events for
            // unrelated sessions on the same server are ignored.
            "session.created" => {
                if let Some(id) = session_id
                    && !self.sessions.contains_key(id)
                    && let Some(parent) = data.get("parentID").and_then(|v| v.as_str())
                    && self.sessions.contains_key(parent)
                {
                    self.sessions.insert(id.to_string(), SessionWork::default());
                }
                None
            }
            "session.deleted" => {
                let id = session_id?;
                if id == self.root {
                    if let Some(work) = self.sessions.get_mut(id) {
                        work.executing = false;
                        let shells = std::mem::take(&mut work.shells);
                        for shell in shells {
                            self.shell_owners.remove(&shell);
                        }
                    }
                } else if let Some(work) = self.sessions.remove(id) {
                    for shell in work.shells {
                        self.shell_owners.remove(&shell);
                    }
                }
                self.pending_permissions.remove(id);
                Some(self.is_waiting())
            }
            // V1's turn-finished signal. (V2 uses the execution events below.)
            "session.idle" => {
                self.set_executing(session_id, false);
                Some(self.is_waiting())
            }
            // V1's turn-started signal.
            "session.status" => {
                let status = data
                    .get("status")
                    .and_then(|s| s.get("type"))
                    .and_then(|t| t.as_str());
                match status {
                    Some("busy") => {
                        self.set_executing(session_id, true);
                        Some(self.is_waiting())
                    }
                    // "idle"/"retry" or anything else: `session.idle` already
                    // covers the idle transition.
                    _ => None,
                }
            }
            // V2's prompt-driven lifecycle. `interrupted` is what a
            // user-aborted turn emits (Esc / cancel) — confirmed live against
            // 2.0.14; there is no `cancelled` outcome. Without it, an
            // interrupted run left the session stuck "working" forever
            // (spinner, caffeinate, idle-timeout all kept alive).
            "session.execution.started" => {
                self.set_executing(session_id, true);
                Some(self.is_waiting())
            }
            "session.execution.succeeded"
            | "session.execution.failed"
            | "session.execution.interrupted" => {
                self.set_executing(session_id, false);
                Some(self.is_waiting())
            }
            // A shell can outlive the turn that started it. Track the running
            // set rather than treating the parent's execution end as "done":
            // a background command (or a Ctrl+B'd foreground one) looks like
            // exactly this.
            "session.shell.started" => {
                if let (Some(id), Some(shell)) = (session_id, shell_id(data)) {
                    let running = data
                        .get("shell")
                        .and_then(|s| s.get("status"))
                        .and_then(|s| s.as_str())
                        .is_none_or(|status| status == "running");
                    if running && let Some(work) = self.sessions.get_mut(id) {
                        work.shells.insert(shell.to_string());
                        self.shell_owners.insert(shell.to_string(), id.to_string());
                    }
                }
                Some(self.is_waiting())
            }
            "session.shell.ended" => {
                self.forget_shell(session_id, shell_id(data));
                Some(self.is_waiting())
            }
            // Shell-domain events on the same stream, without a sessionID.
            "shell.exited" | "shell.deleted" => {
                let Some(shell) = data.get("id").and_then(|v| v.as_str()) else {
                    return Some(self.is_waiting());
                };
                let owner = self.shell_owners.remove(shell);
                if let Some(owner) = owner
                    && let Some(work) = self.sessions.get_mut(&owner)
                {
                    work.shells.remove(shell);
                }
                Some(self.is_waiting())
            }
            // Blocked on a permission prompt / prompt answered.
            "permission.updated" | "permission.asked" => {
                if let Some(id) = session_id {
                    self.pending_permissions.insert(id.to_string());
                }
                Some(true)
            }
            "permission.replied" => {
                if let Some(id) = session_id {
                    self.pending_permissions.remove(id);
                }
                Some(self.is_waiting())
            }
            _ => None,
        }
    }

    fn forget_shell(&mut self, session_id: Option<&str>, shell: Option<&str>) {
        let Some(shell) = shell else {
            return;
        };
        if let Some(id) = session_id
            && let Some(work) = self.sessions.get_mut(id)
        {
            work.shells.remove(shell);
        }
        self.shell_owners.remove(shell);
    }
}

fn shell_id(data: &serde_json::Value) -> Option<&str> {
    data.get("shell")
        .and_then(|shell| shell.get("id"))
        .and_then(|id| id.as_str())
}

fn apply(app: &AppHandle, id: &str, waiting: bool) {
    let registry = app.state::<SessionRegistry>();
    if let Some(info) = registry.apply_hook(id, waiting, None) {
        // A waiting->working (or working->waiting) transition changes
        // whether this session should keep the machine awake.
        crate::commands::session::sync_caffeinate(app);
        let _ = app.emit(
            "session-status",
            StatusEvent {
                id: id.to_string(),
                waiting,
                last_summary: info.last_summary,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "ses_root";

    fn tracker() -> StatusTracker {
        StatusTracker::new(ROOT.to_string())
    }

    /// Legacy (V1) events carry their payload under `properties`.
    fn legacy(kind: &str, properties: serde_json::Value) -> RawEvent {
        RawEvent {
            kind: kind.to_string(),
            properties: Some(properties),
            data: None,
        }
    }

    /// V2 events carry their payload under `data`.
    fn v2(kind: &str, data: serde_json::Value) -> RawEvent {
        RawEvent {
            kind: kind.to_string(),
            properties: None,
            data: Some(data),
        }
    }

    fn child_created(parent: &str, child: &str) -> RawEvent {
        v2(
            "session.created",
            serde_json::json!({ "sessionID": child, "parentID": parent }),
        )
    }

    fn execution(kind: &str, session: &str) -> RawEvent {
        v2(kind, serde_json::json!({ "sessionID": session }))
    }

    fn shell_started(session: &str, shell: &str) -> RawEvent {
        v2(
            "session.shell.started",
            serde_json::json!({
                "sessionID": session,
                "shell": { "id": shell, "status": "running" }
            }),
        )
    }

    fn shell_ended(session: &str, shell: &str) -> RawEvent {
        v2(
            "session.shell.ended",
            serde_json::json!({
                "sessionID": session,
                "shell": { "id": shell, "status": "exited" }
            }),
        )
    }

    #[test]
    fn legacy_idle_for_the_root_means_waiting() {
        let mut t = tracker();
        t.apply(&execution("session.execution.started", ROOT));
        assert_eq!(
            t.apply(&legacy(
                "session.idle",
                serde_json::json!({ "sessionID": ROOT })
            )),
            Some(true)
        );
    }

    #[test]
    fn legacy_idle_for_an_unknown_session_is_ignored() {
        let mut t = tracker();
        assert_eq!(
            t.apply(&legacy(
                "session.idle",
                serde_json::json!({ "sessionID": "ses_someone_else" })
            )),
            None
        );
    }

    #[test]
    fn legacy_status_busy_means_working() {
        let mut t = tracker();
        assert_eq!(
            t.apply(&legacy(
                "session.status",
                serde_json::json!({ "sessionID": ROOT, "status": { "type": "busy" } })
            )),
            Some(false)
        );
    }

    #[test]
    fn legacy_status_idle_has_no_independent_signal() {
        // `session.idle` already covers this transition; `session.status`
        // with a nested "idle"/"retry" type should be a no-op, not a
        // conflicting `Some(true)`.
        let mut t = tracker();
        assert_eq!(
            t.apply(&legacy(
                "session.status",
                serde_json::json!({ "sessionID": ROOT, "status": { "type": "idle" } })
            )),
            None
        );
    }

    #[test]
    fn v2_execution_started_means_working() {
        let mut t = tracker();
        assert_eq!(
            t.apply(&execution("session.execution.started", ROOT)),
            Some(false)
        );
    }

    #[test]
    fn v2_execution_succeeded_means_waiting() {
        let mut t = tracker();
        t.apply(&execution("session.execution.started", ROOT));
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", ROOT)),
            Some(true)
        );
    }

    #[test]
    fn v2_execution_interrupted_means_waiting() {
        let mut t = tracker();
        t.apply(&execution("session.execution.started", ROOT));
        assert_eq!(
            t.apply(&execution("session.execution.interrupted", ROOT)),
            Some(true)
        );
    }

    #[test]
    fn permission_asked_overrides_a_running_turn_until_replied() {
        let mut t = tracker();
        t.apply(&execution("session.execution.started", ROOT));
        assert_eq!(
            t.apply(&v2(
                "permission.asked",
                serde_json::json!({ "sessionID": ROOT })
            )),
            Some(true)
        );
        assert_eq!(
            t.apply(&v2(
                "permission.replied",
                serde_json::json!({ "sessionID": ROOT })
            )),
            Some(false)
        );
    }

    #[test]
    fn legacy_permission_updated_means_waiting() {
        let mut t = tracker();
        t.apply(&execution("session.execution.started", ROOT));
        assert_eq!(
            t.apply(&legacy(
                "permission.updated",
                serde_json::json!({ "sessionID": ROOT })
            )),
            Some(true)
        );
    }

    #[test]
    fn background_child_keeps_session_working_after_root_turn_ends() {
        // The live 2.0.14 sequence for a `background: true` subagent: the
        // parent's execution succeeds while the child is still running.
        let mut t = tracker();
        t.apply(&child_created(ROOT, "ses_child"));
        t.apply(&execution("session.execution.started", ROOT));
        t.apply(&execution("session.execution.started", "ses_child"));
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", ROOT)),
            Some(false),
            "root going idle must not hide a running background subagent"
        );
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", "ses_child")),
            Some(true)
        );
        // The completion then wakes the root into a notification turn; that
        // turn's own lifecycle takes over normally.
        assert_eq!(
            t.apply(&execution("session.execution.started", ROOT)),
            Some(false)
        );
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", ROOT)),
            Some(true)
        );
    }

    #[test]
    fn background_child_starting_while_root_is_idle_makes_it_working() {
        let mut t = tracker();
        t.apply(&child_created(ROOT, "ses_child"));
        assert_eq!(
            t.apply(&execution("session.execution.started", "ses_child")),
            Some(false)
        );
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", "ses_child")),
            Some(true)
        );
    }

    #[test]
    fn nested_subagent_sessions_are_tracked() {
        let mut t = tracker();
        t.apply(&child_created(ROOT, "ses_child"));
        t.apply(&child_created("ses_child", "ses_grandchild"));
        assert_eq!(
            t.apply(&execution("session.execution.started", "ses_grandchild")),
            Some(false)
        );
    }

    #[test]
    fn sessions_not_parented_to_the_root_are_ignored() {
        let mut t = tracker();
        // A session created without a parent (or from an unrelated tree) is
        // not background work for this corral session.
        t.apply(&v2(
            "session.created",
            serde_json::json!({ "sessionID": "ses_other" }),
        ));
        assert_eq!(
            t.apply(&execution("session.execution.started", "ses_other")),
            None
        );
    }

    #[test]
    fn deleting_a_child_forgets_its_running_work() {
        let mut t = tracker();
        t.apply(&child_created(ROOT, "ses_child"));
        t.apply(&execution("session.execution.started", "ses_child"));
        assert_eq!(
            t.apply(&v2(
                "session.deleted",
                serde_json::json!({ "sessionID": "ses_child" })
            )),
            Some(true)
        );
    }

    #[test]
    fn background_shell_keeps_session_working_after_parent_turn_ends() {
        let mut t = tracker();
        t.apply(&execution("session.execution.started", ROOT));
        t.apply(&shell_started(ROOT, "sh_bg"));
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", ROOT)),
            Some(false),
            "a background command outlives the turn that launched it"
        );
        assert_eq!(t.apply(&shell_ended(ROOT, "sh_bg")), Some(true));
    }

    #[test]
    fn top_level_shell_exited_clears_a_running_shell() {
        let mut t = tracker();
        t.apply(&shell_started(ROOT, "sh_bg"));
        assert_eq!(
            t.apply(&v2("shell.exited", serde_json::json!({ "id": "sh_bg" }))),
            Some(true)
        );
    }

    #[test]
    fn background_shell_on_a_child_keeps_session_working() {
        let mut t = tracker();
        t.apply(&child_created(ROOT, "ses_child"));
        t.apply(&execution("session.execution.started", "ses_child"));
        t.apply(&shell_started("ses_child", "sh_child"));
        assert_eq!(
            t.apply(&execution("session.execution.succeeded", "ses_child")),
            Some(false)
        );
        assert_eq!(t.apply(&shell_ended("ses_child", "sh_child")), Some(true));
    }

    #[test]
    fn unknown_event_type_is_ignored() {
        let mut t = tracker();
        assert_eq!(
            t.apply(&v2(
                "message.updated",
                serde_json::json!({ "sessionID": ROOT })
            )),
            None
        );
    }

    #[test]
    fn event_with_no_session_id_is_ignored() {
        let mut t = tracker();
        assert_eq!(
            t.apply(&v2("server.connected", serde_json::json!({}))),
            None
        );
    }
}
