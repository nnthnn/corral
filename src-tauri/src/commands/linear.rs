//! Linear ticket lookup for the "Attach Linear ticket" action.
//!
//! Both commands are `async` + `spawn_blocking` for the same reason as
//! `worktrees::check_log`: `ureq` is synchronous and these are real network
//! calls, so running them inline on the IPC thread would freeze the UI for
//! the duration of the request.

use super::StrErr;

/// Search Linear issues by identifier (`ENG-123`) or free text. Read-only.
#[tauri::command]
pub async fn linear_search(
    api_key: String,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<corral_core::linear::LinearIssueSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        corral_core::linear::search(&api_key, &query, limit.unwrap_or(20))
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}

/// Fetch one Linear issue, including its description, for attaching to the
/// agent. Read-only.
#[tauri::command]
pub async fn linear_issue(
    api_key: String,
    id: String,
) -> Result<corral_core::linear::LinearIssue, String> {
    tauri::async_runtime::spawn_blocking(move || corral_core::linear::issue(&api_key, &id))
        .await
        .map_err(|e| e.to_string())?
        .str_err()
}
