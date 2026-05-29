use std::path::PathBuf;

use tauri::State;

use crate::state::AppState;

/// Sync the user's "默认工作目录" setting (front-end localStorage) into
/// `AppState::user_default_cwd` so the Rust-side cwd resolver can use it.
///
/// Called by the front-end early during startup (before any `create_pane`),
/// and again whenever the user changes the field in SettingsPanel. Empty /
/// missing string clears the override (revert to home / "." fallback).
#[tauri::command]
pub fn set_user_default_cwd(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<(), String> {
    let normalised = path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    *state.user_default_cwd.write() = normalised;
    Ok(())
}
