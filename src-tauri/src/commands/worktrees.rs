//! Worktree lifecycle + git-diff commands — adapters over `corral_core::worktree`
//! and `corral_core::git`.

use corral_core::worktree::{self, Worktree};

use super::StrErr;

/// Resolve a worktree's path from its `(project, feature)` key, converting
/// the error to `String` for the Tauri IPC boundary — the shared first line
/// of `branch_changes`, `file_diff`, and `watch_worktree` below.
fn resolve(project: &str, feature: &str) -> Result<std::path::PathBuf, String> {
    corral_core::config::worktree_path(project, feature).str_err()
}

/// `async` + `spawn_blocking` (see `merge_worktree` below): `worktree::list`
/// runs a full `git status` scan for every worktree across every project, and
/// a blocking command runs inline on the GTK main-loop thread — so a large
/// repo / many worktrees froze the UI on every refresh. The (cheap) PR-cache
/// lookup stays on the calling thread once the scan returns.
#[tauri::command]
pub async fn list_worktrees(
    pr_cache: tauri::State<'_, crate::pr_poll::PrPollCache>,
) -> Result<Vec<Worktree>, String> {
    let mut worktrees = tauri::async_runtime::spawn_blocking(worktree::list)
        .await
        .map_err(|e| e.to_string())?
        .str_err()?;
    for wt in &mut worktrees {
        let id = corral_core::agent_session::key(&wt.project, &wt.feature);
        if let Some(cached) = pr_cache.get(&id) {
            wt.pr_state = Some(cached.info);
            wt.pr_checks = cached.checks;
        }
    }
    Ok(worktrees)
}

#[tauri::command]
pub fn set_worktree_note(project: String, feature: String, note: String) -> Result<(), String> {
    corral_core::worktree_notes::set(&corral_core::agent_session::key(&project, &feature), &note)
        .str_err()
}

#[tauri::command]
pub fn branch_changes(
    project: String,
    feature: String,
) -> Result<corral_core::git::BranchChanges, String> {
    let path = resolve(&project, &feature)?;
    corral_core::git::branch_changes(&path).str_err()
}

#[tauri::command]
pub fn file_diff(project: String, feature: String, path: String) -> Result<String, String> {
    let wt = resolve(&project, &feature)?;
    corral_core::git::file_diff(&wt, &path).str_err()
}

/// Discard uncommitted changes to a single file (restoring it to `HEAD`, or
/// deleting it if untracked/new). Destructive — the frontend confirms with the
/// user before calling this.
#[tauri::command]
pub fn revert_file(
    project: String,
    feature: String,
    path: String,
    old_path: Option<String>,
) -> Result<(), String> {
    let wt = resolve(&project, &feature)?;
    corral_core::git::revert_file(&wt, &path, old_path.as_deref()).str_err()
}

#[tauri::command]
pub fn list_worktree_dir(
    project: String,
    feature: String,
    path: String,
) -> Result<Vec<corral_core::files::FileEntry>, String> {
    let wt = resolve(&project, &feature)?;
    corral_core::files::list_dir(&wt, &path).str_err()
}

#[tauri::command]
pub fn read_worktree_file(
    project: String,
    feature: String,
    path: String,
) -> Result<corral_core::files::FileContent, String> {
    let wt = resolve(&project, &feature)?;
    corral_core::files::read_file(&wt, &path).str_err()
}

/// Re-fetches just one worktree's `GitStatus` — the lightweight counterpart
/// to `list_worktrees` (which recomputes status for every worktree across
/// every project). Used by `worktree_watch.rs`'s file-watcher event so a
/// commit made inside the currently-selected worktree updates its
/// merge/PR-button state live, without the cost of a full reload.
/// `Ok(None)` (rather than an error) if the path isn't a readable git repo,
/// matching `Worktree.status`'s existing best-effort semantics.
/// `async` + `spawn_blocking` (see `merge_worktree` below): `git::status` walks
/// and stats the working tree, and this is invoked from the file-watcher on
/// every relevant fs event — running it inline on the GTK main thread made
/// file changes in a large worktree cause UI jank.
#[tauri::command]
pub async fn refresh_worktree_status(
    project: String,
    feature: String,
) -> Result<Option<corral_core::git::GitStatus>, String> {
    let path = resolve(&project, &feature)?;
    tauri::async_runtime::spawn_blocking(move || corral_core::git::status(&path).ok())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn watch_worktree(
    state: tauri::State<crate::worktree_watch::WorktreeWatch>,
    project: String,
    feature: String,
) -> Result<(), String> {
    let root = resolve(&project, &feature)?;
    state.switch(project, feature, root);
    Ok(())
}

/// `async` + `spawn_blocking` for the same reason as `merge_worktree` below:
/// `worktree::create` does a best-effort network `git fetch` before branching
/// (see `worktree::create_under`), and Tauri runs a *non-async* command inline
/// on the GTK main-loop thread — so a slow or unreachable remote froze the
/// whole UI for the duration of the fetch. `spawn_blocking` moves the git work
/// onto tokio's blocking pool and frees the main thread to keep painting.
#[tauri::command]
pub async fn create_worktree(
    app: tauri::AppHandle,
    provision_registry: tauri::State<'_, crate::commands::provision::Registry>,
    project: String,
    feature: Option<String>,
) -> Result<Worktree, String> {
    let feature = match feature.filter(|f| !f.is_empty()) {
        Some(f) => f,
        None => corral_core::names::generate(&project).str_err()?,
    };
    let project_settings = corral_core::project_settings::get(&project).str_err()?;
    let setup_command = project_settings.setup_command.trim().to_string();
    let key = corral_core::agent_session::key(&project, &feature);

    let create_project = project.clone();
    let create_feature = feature.clone();
    let wt = tauri::async_runtime::spawn_blocking(move || {
        worktree::create(&create_project, &create_feature)
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()?;

    if !setup_command.is_empty() {
        crate::commands::provision::start(app, &provision_registry, key, &wt.path, setup_command);
    }
    Ok(wt)
}

/// `async` + `spawn_blocking` for the same reason as `merge_worktree` below:
/// `worktree::remove` now shells out to real `git push` (network) and runs
/// opencode's `session list`/`session delete` to clear the harness's session
/// store, so it must not run on the GTK main-loop thread. The in-memory agent-session,
/// provision, and run registries are stopped first (fast, on the calling
/// thread) before any `.await`, since `State` handles can't be held across an
/// await point — and the running agent must be killed before its worktree
/// directory disappears out from under it.
#[tauri::command]
pub async fn remove_worktree(
    session_registry: tauri::State<'_, crate::commands::session::SessionRegistry>,
    provision_registry: tauri::State<'_, crate::commands::provision::Registry>,
    run_registry: tauri::State<'_, crate::commands::run::Registry>,
    project: String,
    feature: String,
    delete_branch: bool,
) -> Result<(), String> {
    let key = corral_core::agent_session::key(&project, &feature);
    crate::commands::session::kill_session_best_effort(&session_registry, &key);
    crate::commands::provision::stop_and_remove(&provision_registry, &key);
    crate::commands::run::stop_and_remove(&run_registry, &key);

    let joined = tauri::async_runtime::spawn_blocking(move || {
        worktree::remove(&project, &feature, delete_branch)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

#[tauri::command]
pub fn rename_worktree(
    project: String,
    feature: String,
    new_name: String,
) -> Result<Worktree, String> {
    worktree::rename(&project, &feature, &new_name).str_err()
}

/// `async` + `spawn_blocking` rather than a plain synchronous command: Tauri
/// runs blocking commands inline on whatever thread dispatches the IPC
/// message (see `body_blocking` in `tauri-macros`, which calls the command
/// directly with no thread hop) — on Linux that's the same GTK main loop
/// thread the webview paints on. `merge_into_base` shells out to real `git`
/// (network fetch + push), so without this the whole UI freezes — no
/// repaints, no click handling, nothing — until the rebase/push completes.
/// `spawn_blocking` moves that work onto tokio's dedicated blocking-thread
/// pool, freeing the main thread to keep rendering while it runs.
#[tauri::command]
pub async fn merge_worktree(
    project: String,
    feature: String,
) -> Result<corral_core::merge::MergeResult, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        corral_core::merge::merge_into_base(&project, &feature)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — same reasoning: `rebase_onto_base`
/// fetches from `origin`, a real network call that must not block the main
/// thread.
#[tauri::command]
pub async fn rebase_worktree(
    project: String,
    feature: String,
) -> Result<corral_core::merge::RebaseResult, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        corral_core::merge::rebase_worktree(&project, &feature)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — same reasoning: `gh pr view` is a
/// real network call to the GitHub API and must not block the main thread.
#[tauri::command]
pub async fn pr_status(
    project: String,
    feature: String,
) -> Result<Option<corral_core::pr::PrInfo>, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let path = corral_core::config::worktree_path(&project, &feature)?;
        corral_core::pr::pr_status(&path)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — same reasoning: `gh pr checks` is a
/// real network call and must not block the main thread.
#[tauri::command]
pub async fn pr_checks(
    project: String,
    feature: String,
) -> Result<Vec<corral_core::checks::CheckRun>, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let path = corral_core::config::worktree_path(&project, &feature)?;
        corral_core::checks::pr_checks(&path)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — `gh pr view` is a real network call.
#[tauri::command]
pub async fn pr_details(
    project: String,
    feature: String,
) -> Result<Option<corral_core::checks::PrDetails>, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let path = corral_core::config::worktree_path(&project, &feature)?;
        corral_core::checks::pr_details(&path)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — `gh run view` is a real network call.
#[tauri::command]
pub async fn check_log(
    project: String,
    feature: String,
    check_url: String,
) -> Result<String, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let path = corral_core::config::worktree_path(&project, &feature)?;
        corral_core::checks::check_log(&path, &check_url)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — `gh pr merge` is a real network call.
#[tauri::command]
pub async fn merge_pr(
    project: String,
    feature: String,
) -> Result<corral_core::checks::MergePrResult, String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let path = corral_core::config::worktree_path(&project, &feature)?;
        corral_core::checks::merge_pr(&path)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}

/// See `merge_worktree`'s doc comment — `gh pr ready` is a real network call.
#[tauri::command]
pub async fn mark_pr_ready(project: String, feature: String) -> Result<(), String> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let path = corral_core::config::worktree_path(&project, &feature)?;
        corral_core::checks::mark_pr_ready(&path)
    })
    .await
    .map_err(|e| e.to_string())?;
    joined.str_err()
}
