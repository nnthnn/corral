//! One-shot post-create setup commands (e.g. `pnpm install`), run in the
//! background after a worktree is created. Thin wrapper over
//! `pty_process`'s shared one-shot-PTY core — see its module docs for the
//! streaming/registry mechanics. Output streams to the frontend via
//! `provision-output`/`provision-exit` events and is retained in memory (not
//! persisted — an unfinished run left behind by a killed app is no
//! different from closing a terminal mid-command) so a UI panel can
//! reattach with `get_provision_output` after navigating away and back.

use std::path::Path;

use tauri::{AppHandle, State};

use super::StrErr;
use super::pty_process::{self, PtyProcessState};

const EVENT_PREFIX: &str = "provision";

/// Registry of provisioning runs, keyed like `agent_session::key`
/// (`<project>/<feature>`). Held as Tauri managed state. Wraps
/// `pty_process::Registry` in its own type so Tauri's `State<T>` extraction
/// distinguishes it from `run::Registry`, even though both are backed by
/// the same underlying mechanism.
#[derive(Default)]
pub struct Registry(pty_process::Registry);

pub type ProvisionState = PtyProcessState;

/// Spawn `command` via `sh -c` in `cwd`, tracked in `registry` under `key`.
/// See `pty_process::start` for the streaming/registry mechanics. Best-
/// effort: a spawn failure is logged and surfaced as a synthetic failed run
/// rather than propagated, since a bad setup command should never fail
/// worktree creation itself.
pub fn start(app: AppHandle, registry: &Registry, key: String, cwd: &Path, command: String) {
    pty_process::start(app, &registry.0, key, cwd, command, EVENT_PREFIX);
}

/// Kill a still-running setup command (if any) and drop its registry
/// entry — called from `remove_worktree` so a deleted worktree doesn't
/// leave an orphaned process writing into a directory that no longer
/// exists. A no-op if `key` has no run (already finished and dismissed, or
/// never provisioned).
pub fn stop_and_remove(registry: &Registry, key: &str) {
    pty_process::stop_and_remove(&registry.0, key);
}

#[tauri::command]
pub fn stop_provision(registry: State<Registry>, key: String) -> Result<(), String> {
    pty_process::stop(&registry.0, &key)
}

/// Re-run this worktree's configured setup command from scratch, discarding
/// any previous output — used by the provisioning panel's "Re-run" button
/// (e.g. after fixing whatever caused a failure, or just to redo setup after
/// external changes). Errors if the project has no `setup_command`
/// configured, since there'd be nothing to run.
#[tauri::command]
pub fn rerun_provision(
    app: AppHandle,
    registry: State<Registry>,
    project: String,
    feature: String,
) -> Result<(), String> {
    let settings = corral_core::project_settings::get(&project).str_err()?;
    if settings.setup_command.trim().is_empty() {
        return Err("no setup command configured for this project".into());
    }
    let path = corral_core::config::worktree_path(&project, &feature).str_err()?;
    let key = corral_core::agent_session::key(&project, &feature);
    start(app, &registry, key, &path, settings.setup_command);
    Ok(())
}

#[tauri::command]
pub fn get_provision_output(registry: State<Registry>, key: String) -> Option<ProvisionState> {
    pty_process::get_output(&registry.0, &key)
}

/// Resize a running setup command's PTY to match the frontend's terminal
/// viewer. See `pty_process::resize`.
#[tauri::command]
pub fn resize_provision(
    registry: State<Registry>,
    key: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    pty_process::resize(&registry.0, &key, rows, cols)
}
