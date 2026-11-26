//! Linear ticket lookup for the "Attach Linear ticket" action.
//!
//! All commands are `async` + `spawn_blocking` for the same reason as
//! `worktrees::check_log`: `ureq` is synchronous and these are real network
//! calls, so running them inline on the IPC thread would freeze the UI for
//! the duration of the request.
//!
//! The read commands take no API key: it's a credential, and the webview never
//! receives it (`Settings::linear_api_key` is `#[serde(skip)]`, so it crosses
//! the IPC boundary in neither direction). Each loads it from the config dir
//! through `corral_core::settings::load` — the same place `get_settings`
//! reads — and `linear_set_api_key` is the only way a new one gets in.

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

/// Store the Linear API key typed into Settings.
///
/// The key is its own argument rather than a field on the `save_settings`
/// blob: `Settings.linear_api_key` is `#[serde(skip)]`, so the whole-settings
/// round trip can't carry it in either direction, and the credential has
/// exactly one write path. Trimmed; blank is a no-op (removal is
/// `linear_clear_api_key`'s job), so an empty submit can't wipe a stored key.
#[tauri::command]
pub async fn linear_set_api_key(key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if key.trim().is_empty() {
            return Ok(());
        }
        settings::set_linear_api_key(&key)
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}

/// Forget the stored Linear API key — the explicit clear behind the Settings
/// field's "Remove key" button.
#[tauri::command]
pub async fn linear_clear_api_key() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(settings::clear_linear_api_key)
        .await
        .map_err(|e| e.to_string())?
        .str_err()
}
