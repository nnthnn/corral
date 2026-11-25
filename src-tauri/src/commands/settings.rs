//! App-settings commands — adapters over `corral_core::settings`.

use super::StrErr;

#[tauri::command]
pub fn get_settings() -> Result<corral_core::settings::Settings, String> {
    corral_core::settings::load().str_err()
}

#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    settings: corral_core::settings::Settings,
) -> Result<(), String> {
    corral_core::settings::save(&settings).str_err()?;
    super::session::sync_caffeinate(&app);
    Ok(())
}

/// Where the Linear API key is actually stored — "your system keychain" or
/// the owner-only file fallback. Settings shows this instead of claiming the
/// stronger guarantee on machines that don't have a keychain daemon.
#[tauri::command]
pub fn linear_key_storage() -> Result<String, String> {
    Ok(corral_core::settings::linear_api_key_storage_description()
        .str_err()?
        .to_string())
}
