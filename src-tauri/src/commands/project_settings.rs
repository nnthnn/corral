//! Per-project provisioning-rule commands — adapters over
//! `corral_core::project_settings`.

use super::StrErr;

#[tauri::command]
pub fn get_project_settings(
    project: String,
) -> Result<corral_core::project_settings::ProjectSettings, String> {
    corral_core::project_settings::get(&project).str_err()
}

#[tauri::command]
pub fn save_project_settings(
    project: String,
    settings: corral_core::project_settings::ProjectSettings,
) -> Result<(), String> {
    corral_core::project_settings::set(&project, &settings).str_err()
}
