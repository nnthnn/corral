//! Shared one-shot PTY-backed process core, used by both `provision.rs`
//! (Setup) and `run.rs` (the Run tab) — spawn a command in a PTY, stream its
//! merged stdout+stderr to the frontend, and buffer it so a UI panel can
//! reattach after navigating away and back. Neither caller exposes stdin;
//! this is a fire-and-forget mechanism, not `session.rs`'s interactive
//! agent-session PTY.
//!
//! Run in a real PTY (like `session.rs`'s agent sessions), not a plain pipe.
//! Two reasons: most CLIs (cargo, npm, pnpm...) detect a non-tty stdout and
//! both disable color output *and* switch from line-buffered to fully
//! block-buffered writes — the latter means chunks of output only surface
//! once a libc buffer fills or the process exits, which reads as "output
//! goes missing" for anything long-running (e.g. a `cargo build` interleaved
//! with a `vite build`). A PTY makes the child believe it's talking to a
//! real terminal, restoring both proper flushing and color.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::StrErr;

/// Generous fixed size for a command's PTY — resized to the actual viewer
/// once the frontend mounts its terminal (see [`resize`]).
const INITIAL_PTY_SIZE: PtySize = PtySize {
    rows: 40,
    cols: 120,
    pixel_width: 0,
    pixel_height: 0,
};

#[derive(Default)]
struct Inner {
    output: Vec<u8>,
    running: bool,
    exit_code: Option<i32>,
}

struct Run {
    /// `None` once killed via [`stop_and_remove`] or after a failed spawn —
    /// there's nothing left to wait on or kill.
    child: Mutex<Option<Box<dyn Child + Send + Sync>>>,
    /// Kept alive so [`resize`] can resize it, and so the PTY doesn't close
    /// out from under the reader thread. `None` after a failed spawn, same
    /// as `child`.
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    inner: Mutex<Inner>,
}

/// Record a spawn failure as a synthetic finished run, so it shows up in the
/// UI like any other completed (failed) command instead of silently doing
/// nothing.
fn failed_run(
    app: &AppHandle,
    registry: &Registry,
    key: String,
    event_prefix: &str,
    message: String,
) {
    tracing::warn!("{event_prefix}: {message}");
    let run = Arc::new(Run {
        child: Mutex::new(None),
        master: Mutex::new(None),
        inner: Mutex::new(Inner {
            output: message.into_bytes(),
            running: false,
            exit_code: None,
        }),
    });
    registry.runs.lock().insert(key.clone(), run);
    let _ = app.emit(
        &format!("{event_prefix}-exit"),
        ExitEvent { key, code: None },
    );
}

/// Registry of one-shot PTY runs, keyed like `agent_session::key`
/// (`<project>/<feature>`). `provision::Registry` and `run::Registry` each
/// wrap their own instance of this type, so Setup and Run never collide
/// even though both key by the same worktree id — Tauri's `State<T>`
/// extraction is by type, so the two newtypes are distinct managed state.
#[derive(Default)]
pub struct Registry {
    runs: Mutex<HashMap<String, Arc<Run>>>,
}

#[derive(Clone, Serialize)]
pub struct PtyProcessState {
    pub output: Vec<u8>,
    pub running: bool,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Serialize)]
struct OutputEvent {
    key: String,
    #[serde(with = "super::base64_bytes")]
    data: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct ExitEvent {
    key: String,
    code: Option<i32>,
}

/// Read `reader` to EOF, calling `on_chunk` for each chunk read. Split out
/// from [`start`]'s background thread so the streaming behavior is testable
/// against a plain `impl Read` (e.g. a spawned process's stdout) without a
/// Tauri `AppHandle`.
fn stream_output(mut reader: impl Read, mut on_chunk: impl FnMut(&[u8])) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Err(e) => {
                tracing::warn!("pty_process: read error: {e}");
                break;
            }
            Ok(n) => on_chunk(&buf[..n]),
        }
    }
}

/// Spawn `command` via `sh -c` in `cwd`, tracked in `registry` under `key`.
/// Streams merged stdout+stderr to the frontend via `<event_prefix>-output`
/// events (and buffers it for [`get_output`] reattach), then emits
/// `<event_prefix>-exit` once the process ends. Best-effort: a spawn
/// failure is logged and surfaced as a synthetic failed run rather than
/// propagated, since a bad command should never fail the caller's own
/// workflow (worktree creation for Setup, clicking Run for the Run tab).
pub fn start(
    app: AppHandle,
    registry: &Registry,
    key: String,
    cwd: &Path,
    command: String,
    event_prefix: &'static str,
) {
    let pair = match native_pty_system().openpty(INITIAL_PTY_SIZE) {
        Ok(pair) => pair,
        Err(e) => {
            failed_run(
                &app,
                registry,
                key,
                event_prefix,
                format!("failed to open pty: {e}"),
            );
            return;
        }
    };

    let mut cmd = CommandBuilder::new("sh");
    cmd.args(["-c", &command]);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");

    let child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            drop(pair.slave);
            failed_run(
                &app,
                registry,
                key,
                event_prefix,
                format!("failed to start command: {e}"),
            );
            return;
        }
    };
    // Drop the slave handle so the reader sees EOF once the command exits.
    drop(pair.slave);

    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(e) => {
            let mut child = child;
            let _ = child.kill();
            failed_run(
                &app,
                registry,
                key,
                event_prefix,
                format!("failed to read command output: {e}"),
            );
            return;
        }
    };

    let run = Arc::new(Run {
        child: Mutex::new(Some(child)),
        master: Mutex::new(Some(pair.master)),
        inner: Mutex::new(Inner {
            output: Vec::new(),
            running: true,
            exit_code: None,
        }),
    });
    registry.runs.lock().insert(key.clone(), run.clone());

    std::thread::spawn(move || {
        let output_event = format!("{event_prefix}-output");
        stream_output(reader, |chunk| {
            super::append_capped(
                &mut run.inner.lock().output,
                chunk,
                super::MAX_PTY_OUTPUT_BUFFER,
            );
            let _ = app.emit(
                &output_event,
                OutputEvent {
                    key: key.clone(),
                    data: chunk.to_vec(),
                },
            );
        });
        let code = {
            let mut child_slot = run.child.lock();
            match child_slot.as_mut() {
                // `None` here means `stop_and_remove` already killed and
                // dropped it while we were streaming.
                Some(child) => child
                    .wait()
                    .ok()
                    .and_then(|status| status.exit_code().try_into().ok()),
                None => None,
            }
        };
        {
            let mut inner = run.inner.lock();
            inner.running = false;
            inner.exit_code = code;
        }
        tracing::info!("{event_prefix}: command for {key} exited (code={code:?})");
        let _ = app.emit(&format!("{event_prefix}-exit"), ExitEvent { key, code });
    });
}

/// Kill a still-running command (if any) and drop its registry entry —
/// called from `remove_worktree` so a deleted worktree doesn't leave an
/// orphaned process writing into a directory that no longer exists. A no-op
/// if `key` has no run (already finished and dismissed, or never started).
pub fn stop_and_remove(registry: &Registry, key: &str) {
    if let Some(run) = registry.runs.lock().remove(key)
        && let Some(child) = run.child.lock().as_mut()
    {
        let _ = child.kill();
    }
}

/// Kill a still-running command without removing it from the registry —
/// unlike [`stop_and_remove`] (used when the worktree itself is being
/// deleted), this leaves the buffered output and the soon-to-arrive exit
/// event in place so the panel keeps showing what ran and why it stopped. A
/// no-op if `key` has no run or it already finished.
pub fn stop(registry: &Registry, key: &str) -> Result<(), String> {
    if let Some(run) = registry.runs.lock().get(key)
        && let Some(child) = run.child.lock().as_mut()
    {
        child.kill().str_err()?;
    }
    Ok(())
}

/// Retained output/running/exit-code state for `key`, or `None` if it has
/// no run — used both for a UI panel's initial reattach fetch and (in
/// principle) any other caller wanting a point-in-time snapshot.
pub fn get_output(registry: &Registry, key: &str) -> Option<PtyProcessState> {
    registry.runs.lock().get(key).map(|run| {
        let inner = run.inner.lock();
        PtyProcessState {
            output: inner.output.clone(),
            running: inner.running,
            exit_code: inner.exit_code,
        }
    })
}

/// Resize a running command's PTY to match the frontend's terminal viewer,
/// so wrapping/progress-bar rendering matches what's actually visible. A
/// no-op if `key` has no run, or its run already exited (no PTY left to
/// resize) — callers resize best-effort on mount/resize without checking
/// whether the command already finished.
pub fn resize(registry: &Registry, key: &str, rows: u16, cols: u16) -> Result<(), String> {
    let runs = registry.runs.lock();
    let Some(run) = runs.get(key) else {
        return Ok(());
    };
    if let Some(master) = run.master.lock().as_ref() {
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .str_err()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn stream_output_collects_every_chunk_until_eof() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("echo hi")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut collected = Vec::new();

        stream_output(stdout, |chunk| collected.extend_from_slice(chunk));
        let status = child.wait().unwrap();

        assert_eq!(collected, b"hi\n");
        assert!(status.success());
    }

    fn spawn_pty(command: &str) -> (Box<dyn Child + Send + Sync>, Box<dyn MasterPty + Send>) {
        let pair = native_pty_system().openpty(INITIAL_PTY_SIZE).unwrap();
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", command]);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        (child, pair.master)
    }

    #[test]
    fn stop_and_remove_kills_a_running_child_and_drops_the_entry() {
        let registry = Registry::default();
        let (child, master) = spawn_pty("sleep 5");
        let run = Arc::new(Run {
            child: Mutex::new(Some(child)),
            master: Mutex::new(Some(master)),
            inner: Mutex::new(Inner {
                output: Vec::new(),
                running: true,
                exit_code: None,
            }),
        });
        registry.runs.lock().insert("proj/feat".to_string(), run);

        stop_and_remove(&registry, "proj/feat");

        assert!(registry.runs.lock().get("proj/feat").is_none());
    }

    #[test]
    fn stop_and_remove_on_an_unknown_key_is_a_noop() {
        let registry = Registry::default();
        stop_and_remove(&registry, "no/such-key");
    }

    #[test]
    fn stop_kills_the_child_but_keeps_the_registry_entry() {
        let registry = Registry::default();
        let (child, master) = spawn_pty("sleep 5");
        let run = Arc::new(Run {
            child: Mutex::new(Some(child)),
            master: Mutex::new(Some(master)),
            inner: Mutex::new(Inner {
                output: Vec::new(),
                running: true,
                exit_code: None,
            }),
        });
        registry.runs.lock().insert("proj/feat".to_string(), run);

        stop(&registry, "proj/feat").unwrap();

        assert!(registry.runs.lock().get("proj/feat").is_some());
    }

    #[test]
    fn stop_on_an_unknown_key_is_a_noop() {
        let registry = Registry::default();
        stop(&registry, "no/such-key").unwrap();
    }

    #[test]
    fn get_output_returns_none_for_an_unknown_key() {
        let registry = Registry::default();
        assert!(get_output(&registry, "no/such-key").is_none());
    }

    #[test]
    fn get_output_returns_a_snapshot_of_retained_state() {
        let registry = Registry::default();
        let run = Arc::new(Run {
            child: Mutex::new(None),
            master: Mutex::new(None),
            inner: Mutex::new(Inner {
                output: b"hi".to_vec(),
                running: false,
                exit_code: Some(0),
            }),
        });
        registry.runs.lock().insert("proj/feat".to_string(), run);

        let state = get_output(&registry, "proj/feat").unwrap();
        assert_eq!(state.output, b"hi");
        assert!(!state.running);
        assert_eq!(state.exit_code, Some(0));
    }
}
