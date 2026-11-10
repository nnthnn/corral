//! Long-lived "Run" command per worktree (e.g. `pnpm dev`), shown in its own
//! tab next to Agent/Diff. Thin wrapper over `pty_process`'s shared
//! one-shot-PTY core — see its module docs for the streaming/registry
//! mechanics this reuses verbatim from `provision.rs`. Unlike Setup, a Run
//! process is expected to be long-lived (a dev server); it's only ever
//! started/stopped by explicit user action in the Run tab, never
//! auto-started or tied to the agent session's own lifecycle.

use tauri::{AppHandle, State};

use super::StrErr;
use super::pty_process::{self, PtyProcessState};
use super::run_detect::detect_run_command;

const EVENT_PREFIX: &str = "run";

/// Registry of Run-tab processes, keyed like `agent_session::key`
/// (`<project>/<feature>`). Held as Tauri managed state, distinct from
/// `provision::Registry` even though both wrap `pty_process::Registry` —
/// Tauri's `State<T>` extraction is by type, so Setup and Run never share a
/// process or collide on key.
#[derive(Default)]
pub struct Registry(pty_process::Registry);

pub type RunState = PtyProcessState;

/// Kill a still-running Run process (if any) and drop its registry entry —
/// called from `remove_worktree` alongside `provision::stop_and_remove`, so
/// a deleted worktree doesn't leave an orphaned dev server behind.
pub fn stop_and_remove(registry: &Registry, key: &str) {
    pty_process::stop_and_remove(&registry.0, key);
}

/// Start (or restart) this worktree's Run command. Persists `command` as
/// the project's `run_command` override whenever it differs from what's
/// already saved, so the next time the Run tab is opened it prefills with
/// what was actually run rather than re-guessing.
#[tauri::command]
pub fn start_run(
    app: AppHandle,
    registry: State<Registry>,
    project: String,
    feature: String,
    command: String,
) -> Result<(), String> {
    let mut settings = corral_core::project_settings::get(&project).str_err()?;
    if settings.run_command != command {
        settings.run_command = command.clone();
        corral_core::project_settings::set(&project, &settings).str_err()?;
    }
    let cwd = corral_core::config::worktree_path(&project, &feature).str_err()?;
    let key = corral_core::agent_session::key(&project, &feature);
    pty_process::start(app, &registry.0, key, &cwd, command, EVENT_PREFIX);
    Ok(())
}

#[tauri::command]
pub fn stop_run(registry: State<Registry>, key: String) -> Result<(), String> {
    pty_process::stop(&registry.0, &key)
}

#[tauri::command]
pub fn get_run_output(registry: State<Registry>, key: String) -> Option<RunState> {
    pty_process::get_output(&registry.0, &key)
}

/// Resize a running Run command's PTY to match the frontend's terminal
/// viewer. See `pty_process::resize`.
#[tauri::command]
pub fn resize_run(
    registry: State<Registry>,
    key: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    pty_process::resize(&registry.0, &key, rows, cols)
}

/// Resolve the command the Run tab should prefill for this worktree: the
/// project's saved `run_command` override if one exists, else a best-effort
/// auto-detected guess (see `run_detect`), else an empty string meaning
/// "nothing to suggest, wait for the user to type one".
#[tauri::command]
pub fn resolve_run_command(project: String, feature: String) -> Result<String, String> {
    let settings = corral_core::project_settings::get(&project).str_err()?;
    if !settings.run_command.trim().is_empty() {
        return Ok(settings.run_command);
    }
    let cwd = corral_core::config::worktree_path(&project, &feature).str_err()?;
    Ok(detect_run_command(&cwd).unwrap_or_default())
}
