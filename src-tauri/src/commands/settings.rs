//! App-settings commands — adapters over `corral_core::settings`.

use super::StrErr;

/// `async` + `spawn_blocking`: `settings::load` reads `settings.json` *and*
/// pulls the Linear key out of the OS store (`secret::get` — a Secret Service
/// round trip on Linux, a Keychain call on macOS), and a non-`async` command
/// runs inline on the GTK main thread. An unattended keyring unlock prompt
/// therefore froze every window. See `start_session`'s comment for the
/// dispatch split.
#[tauri::command]
pub async fn get_settings() -> Result<corral_core::settings::Settings, String> {
    tauri::async_runtime::spawn_blocking(corral_core::settings::load)
        .await
        .map_err(|e| e.to_string())?
        .str_err()
}

/// See `get_settings`: the save is a JSON write plus its `.bak`, and
/// `sync_caffeinate` reloads settings (a second keyring read). Both run inside
/// one `spawn_blocking` closure rather than one after the other on the async
/// worker: an `async` command's body *is* a Tokio task, so a blocking call left
/// in it stalls a runtime worker for its duration, and `sync_caffeinate`'s
/// keyring read can wait on an unlock prompt. `sync_caffeinate` only takes
/// locks, reads app state, and starts/stops the sleep inhibitor, so it's
/// thread-agnostic — see `secret::on_keyring_thread`'s doc comment.
#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    settings: corral_core::settings::Settings,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        corral_core::settings::save(&settings)?;
        super::session::sync_caffeinate(&app);
        Ok::<_, corral_core::Error>(())
    })
    .await
    .map_err(|e| e.to_string())?
    .str_err()
}

/// Where the Linear API key is actually stored — "your system keychain" or
/// the owner-only file fallback. Settings shows this instead of claiming the
/// stronger guarantee on machines that don't have a keychain daemon.
///
/// `async` + `spawn_blocking` because answering it probes the OS store
/// (`secret::backend`) — the same keyring round trip `get_settings` makes.
#[tauri::command]
pub async fn linear_key_storage() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(corral_core::settings::linear_api_key_storage_description)
        .await
        .map_err(|e| e.to_string())?
        .map(str::to_string)
        .str_err()
}
