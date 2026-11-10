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
