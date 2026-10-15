//! Settings writes commit through Runtime; legacy pane creation reads a projection.
use tauri::{AppHandle, State};
use crate::state::AppState;
#[tauri::command]
pub fn set_user_default_cwd(_app: AppHandle, state: State<'_, AppState>, path: Option<String>) -> Result<(), String> {
    // Shell-relative paths retain the launching shell's OS context.
    let normalized = ridge_core::commands::settings::normalize_default_cwd(path);
    let normalized = normalized.map(|path| if path.is_absolute() {Ok(path)} else {
        std::env::current_dir().map(|cwd| cwd.join(path)).map_err(|error| error.to_string())
    }).transpose()?;
    super::runtime::request::<()>("set_user_default_cwd", serde_json::json!({"path":normalized}), true,
        super::runtime::window_connection("settings"))?;
    *state.user_default_cwd.write() = normalized;
    Ok(())
}

#[tauri::command]
pub fn get_user_default_cwd(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let value: serde_json::Value = super::runtime::request("get_user_default_cwd", serde_json::json!({}), true,
        super::runtime::window_connection("settings"))?;
    if value["configured"] == true {
        *state.user_default_cwd.write() = ridge_core::commands::settings::normalize_default_cwd(
            value["path"].as_str().map(str::to_string));
    }
    Ok(value)
}
