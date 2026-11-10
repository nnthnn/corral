//! Background thread that listens on this corral-app instance's Unix socket
//! for status callbacks from Claude Code hooks (see `corral_core::hooks`),
//! and forwards them into the session `SessionRegistry` + a `session-status` event
//! — replacing a PTY-output-timing heuristic with Claude Code's own signal
//! for "genuinely waiting on the user" vs "working".

use std::io::BufRead;
use std::os::unix::net::{UnixListener, UnixStream};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::session::SessionRegistry;

#[derive(Deserialize)]
struct HookMessage {
    id: String,
    event: String,
    /// Present only on a `Stop` hook whose transcript parsed cleanly — see
    /// `corral-cli`'s `try_notify_hook`.
    #[serde(default)]
    summary: Option<String>,
}

/// Shared with `opencode_events.rs`, which emits the same `session-status`
/// event shape from opencode's SSE stream instead of Claude Code's hook
/// socket — see that module's doc comment for why.
#[derive(Clone, Serialize)]
pub(crate) struct StatusEvent {
    pub(crate) id: String,
    pub(crate) waiting: bool,
    pub(crate) last_summary: Option<String>,
}

/// Bind this instance's hook socket and process messages for the lifetime
/// of the process. Failure to bind (e.g. an unwritable runtime dir) is
/// logged and non-fatal — sessions simply keep no status signal, same as
/// before this feature existed.
pub fn spawn(app: AppHandle) {
    let path = corral_core::hooks::socket_path();
    // Clean up a stale socket file from a prior crash before binding.
    let _ = std::fs::remove_file(&path);
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        tracing::warn!("hook_listener: failed to create {dir:?}: {e}");
        return;
    }
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("hook_listener: failed to bind {path:?}: {e}");
            return;
        }
    };
    tracing::info!("hook_listener: listening on {path:?}");

    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let app = app.clone();
            std::thread::spawn(move || handle_connection(&app, conn));
        }
    });
}

fn handle_connection(app: &AppHandle, conn: UnixStream) {
    let reader = std::io::BufReader::new(conn);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<HookMessage>(&line) else {
            tracing::debug!("hook_listener: ignoring unparseable message: {line}");
            continue;
        };
        let waiting = match msg.event.as_str() {
            "waiting" => true,
            "working" => false,
            other => {
                tracing::debug!("hook_listener: ignoring unknown event {other:?}");
                continue;
            }
        };
        let registry = app.state::<SessionRegistry>();
        if let Some(info) = registry.apply_hook(&msg.id, waiting, msg.summary) {
            // A waiting->working (or working->waiting) transition changes
            // whether this session should keep the machine awake.
            crate::commands::session::sync_caffeinate(app);
            let _ = app.emit(
                "session-status",
                StatusEvent {
                    id: msg.id,
                    waiting,
                    last_summary: info.last_summary,
                },
            );
        }
    }
}
