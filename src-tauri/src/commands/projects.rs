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

/// Create and register a brand-new project: a directory at `parent/name`
/// (`parent` defaults to `~/source` when blank) initialized as a git repo with
/// one empty commit, so corral can create its first worktree immediately.
/// Returns the project name. The empty commit is what makes the very first
/// worktree possible — `git worktree add` needs a commit to branch from — while
/// leaving all actual scaffolding to the agent that runs there.
///
/// `async` + `spawn_blocking` for the same reason as `create_worktree`:
/// a non-async command runs inline on the GTK main thread, and this does real
/// disk work (`git init` + commit) plus a keychain read, since registering
/// reloads settings — which froze the whole UI for its duration.
#[tauri::command]
pub async fn create_project(parent: String, name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("project name can't be empty".to_string());
    }
    let create_name = name.clone();
    tauri::async_runtime::spawn_blocking(move || {
        corral_core::project_create::create(&create_name, &parent)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(project_create_error)?;
    Ok(name)
}

/// The actionable copy for `project_create`'s two expected failures.
fn project_create_error(e: corral_core::Error) -> String {
    match e {
        corral_core::Error::ProjectDirNotEmpty(dir) => format!(
            "{} already exists and isn't empty — pick a different name or location, or use Add project to register it",
            dir.display()
        ),
        // `create` validates names in `project_create`; give the dialog the
        // same actionable copy the non-empty case gets instead of the bare
        // "invalid name: ..." Display string.
        corral_core::Error::InvalidName(_) => "that name can't be used as a folder name — use a single path segment (no slashes, no surrounding spaces)".to_string(),
        e => e.to_string(),
    }
}

/// Register the directory at `path` (an absolute directory, e.g. from the
/// GUI's native folder picker) as a project corral tracks, deriving its name
/// from the directory's basename. A directory that already has `.git` is
/// registered as-is; an empty one is initialized as a fresh repo (same as
/// `create_project`) — there's nothing to import, so that's just a new project
/// whose scaffolding hasn't happened yet. A non-empty non-repo directory is
/// rejected. Errors if a project with that name is already registered.
/// Returns the derived name.
///
/// `async` + `spawn_blocking` for the same reason as `create_project`: the
/// empty-folder case runs `git init` + commit and registering reloads settings
/// (a keyring read), none of which belongs on the GTK main thread.
#[tauri::command]
pub async fn add_project(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || add_project_blocking(path))
        .await
        .map_err(|e| e.to_string())?
}

/// The blocking half of [`add_project`], kept separate so the command can hand
/// it the whole body unchanged.
fn add_project_blocking(path: String) -> Result<String, String> {
    let canonical = std::fs::canonicalize(&path).map_err(|e| format!("{path}: {e}"))?;
    match corral_core::project_create::register_existing(&canonical) {
        Ok(name) => Ok(name),
        Err(corral_core::Error::ProjectDirNotEmpty(dir)) => Err(format!(
            "{} isn't a git repository and isn't empty — pick an existing repo, or an empty folder to start a new project",
            dir.display()
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// Remove `project` from corral entirely: stops any running session,
/// provisioning, or Run-tab process for each of its worktrees, clears their
/// session-store and note-store entries, then unregisters the project
/// (dropping its saved settings too). Nothing on disk is touched — the repo
/// and its worktree directories stay exactly where they are; the project
/// reappears if re-added later via `add_project`.
///
/// `async` + `spawn_blocking` around all three phases, for the reason
/// `merge_worktree` documents: `worktree::list()` runs a full `git status`
/// scan for every worktree of every project (the same call `list_worktrees`
/// was moved off the main thread for), and `unregister` rewrites the registry
/// and reloads settings, which reads the OS keyring. The teardown in between
/// is not free either — the session/note stores are flock-guarded file I/O,
/// and killing a session waits on child processes — and an `async` command's
/// body runs on a Tokio worker, so leaving it there would park that worker.
/// The `State` borrows are the only reason it's split into two closures at
/// all: they can't cross into the first `spawn_blocking` while the command
/// still needs them afterwards.
#[tauri::command]
pub async fn remove_project(
    app: tauri::AppHandle,
    session_registry: tauri::State<'_, crate::commands::session::SessionRegistry>,
    provision_registry: tauri::State<'_, crate::commands::provision::Registry>,
    run_registry: tauri::State<'_, crate::commands::run::Registry>,
    project: String,
) -> Result<(), String> {
    let filter = project.clone();
    let worktrees: Vec<_> = tauri::async_runtime::spawn_blocking(move || {
        worktree::list().map(|all| {
            all.into_iter()
                .filter(|w| w.project == filter)
                .collect::<Vec<_>>()
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()?;

    let session_registry = session_registry.handle();
    let provision_registry = provision_registry.handle();
    let run_registry = run_registry.handle();
    tauri::async_runtime::spawn_blocking(move || {
        for wt in &worktrees {
            let key = corral_core::agent_session::key(&wt.project, &wt.feature);
            crate::commands::session::kill_session_best_effort(&app, &session_registry, &key);
            crate::commands::provision::stop_and_remove(&provision_registry, &key);
            crate::commands::run::stop_and_remove(&run_registry, &key);
            if let Err(e) = corral_core::agent_session::remove(&key) {
                tracing::warn!(
                    "remove_project: failed to clear session store entry for {key}: {e}"
                );
            }
            if let Err(e) = corral_core::worktree_notes::set(&key, "") {
                tracing::warn!("remove_project: failed to clear worktree note for {key}: {e}");
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    tauri::async_runtime::spawn_blocking(move || {
        corral_core::project_settings::unregister(&project)
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}
