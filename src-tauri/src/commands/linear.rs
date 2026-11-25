//! Linear ticket lookup for the "Attach Linear ticket" action.
//!
//! All commands are `async` + `spawn_blocking` for the same reason as
//! `worktrees::check_log`: `ureq` is synchronous and these are real network
//! calls, so running them inline on the IPC thread would freeze the UI for
//! the duration of the request.
//!
//! None of them take the API key as an argument: it's a credential, and the
//! webview never receives it (`Settings::linear_api_key` is `#[serde(skip)]`).
//! Each command loads it from the config dir through
//! `corral_core::settings::load` — the same place `get_settings` reads.

use super::StrErr;

use corral_core::settings;

/// The stored API key, read inside the blocking closure (it's a file read, and
/// keeping it here means it never crosses back into the webview).
fn api_key() -> corral_core::Result<String> {
    settings::load().map(|s| s.linear_api_key)
}

/// Search Linear issues by identifier (`ENG-123`) or free text. Read-only.
#[tauri::command]
pub async fn linear_search(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<corral_core::linear::LinearIssueSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = api_key()?;
        corral_core::linear::search(&key, &query, limit.unwrap_or(20))
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}

/// Recently updated issues, for the dialog's default list. Read-only.
#[tauri::command]
pub async fn linear_recent(
    limit: Option<u32>,
) -> Result<Vec<corral_core::linear::LinearIssueSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = api_key()?;
        corral_core::linear::recent(&key, limit.unwrap_or(10))
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}

/// Fetch one Linear issue, including its description, for attaching to the
/// agent. Read-only.
#[tauri::command]
pub async fn linear_issue(id: String) -> Result<corral_core::linear::LinearIssue, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = api_key()?;
        corral_core::linear::issue(&key, &id)
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}

/// Forget the stored Linear API key — the explicit clear behind the Settings
/// field's "Remove key" button. A blank key saved through `save_settings`
/// deliberately does *not* clear it (the webview's copy is always blank, so
/// blank has to mean "unchanged").
#[tauri::command]
pub async fn linear_clear_api_key() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(settings::clear_linear_api_key)
        .await
        .map_err(|e| e.to_string())?
        .str_err()
}
