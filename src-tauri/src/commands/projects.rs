//! Project-listing and registry-mutation commands — adapters over
//! `corral_core::project`/`corral_core::project_settings`.

use corral_core::{project, worktree};

use super::StrErr;

#[tauri::command]
pub fn list_projects() -> Result<Vec<String>, String> {
    project::list().str_err()
}

/// Best-effort auto-detected project icon as a `data:` URI, or `None` if
/// nothing was found — the frontend falls back to a default glyph either way.
#[tauri::command]
pub fn project_icon(project: String) -> Option<String> {
    project::find_icon(&project)
}

/// Register the git repo at `path` (an absolute directory, e.g. from the
/// GUI's native folder picker) as a project corral tracks, deriving its name
/// from the directory's basename. Errors if `path` isn't a git repo, or if a
/// project with that name is already registered. Returns the derived name.
#[tauri::command]
pub fn add_project(path: String) -> Result<String, String> {
    let canonical = std::fs::canonicalize(&path).map_err(|e| format!("{path}: {e}"))?;
    if !canonical.join(".git").exists() {
        return Err(format!("{} is not a git repository", canonical.display()));
    }
    let name = canonical
        .file_name()
        .ok_or_else(|| format!("{} has no directory name", canonical.display()))?
        .to_string_lossy()
        .into_owned();
    corral_core::project_settings::register(&name, canonical).str_err()?;
    Ok(name)
}

/// Remove `project` from corral entirely: stops any running session,
/// provisioning, or Run-tab process for each of its worktrees, clears their
/// session-store and note-store entries, then unregisters the project
/// (dropping its saved settings too). Nothing on disk is touched — the repo
/// and its worktree directories stay exactly where they are; the project
/// reappears if re-added later via `add_project`.
#[tauri::command]
pub fn remove_project(
    session_registry: tauri::State<crate::commands::session::SessionRegistry>,
    provision_registry: tauri::State<crate::commands::provision::Registry>,
    run_registry: tauri::State<crate::commands::run::Registry>,
    project: String,
) -> Result<(), String> {
    let worktrees: Vec<_> = worktree::list()
        .str_err()?
        .into_iter()
        .filter(|w| w.project == project)
        .collect();
    for wt in &worktrees {
        let key = corral_core::agent_session::key(&wt.project, &wt.feature);
        crate::commands::session::kill_session_best_effort(&session_registry, &key);
        crate::commands::provision::stop_and_remove(&provision_registry, &key);
        crate::commands::run::stop_and_remove(&run_registry, &key);
        if let Err(e) = corral_core::agent_session::remove(&key) {
            tracing::warn!("remove_project: failed to clear session store entry for {key}: {e}");
        }
        if let Err(e) = corral_core::worktree_notes::set(&key, "") {
            tracing::warn!("remove_project: failed to clear worktree note for {key}: {e}");
        }
    }
    corral_core::project_settings::unregister(&project).str_err()
}
