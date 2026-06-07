use tauri::{AppHandle, State};

use crate::state::AppState;

/// Sync the user's "默认工作目录" setting (front-end localStorage) into
/// `AppState::user_default_cwd` so the Rust-side cwd resolver can use it.
///
/// Called by the front-end early during startup (before any `create_pane`),
/// and again whenever the user changes the field in SettingsPanel. Empty /
/// missing string clears the override (revert to home / "." fallback).
///
/// **S1 thin wrapper**: the normalisation + write logic now lives in
/// `ridge_core::commands::settings::set_user_default_cwd`. This command builds
/// a `ridge_core::Ctx` over `AppState` and delegates. Behaviour is unchanged —
/// the wrapper maps the core error back to the legacy `Result<(), String>`
/// shape `#[tauri::command]` serializes.
#[tauri::command]
pub fn set_user_default_cwd(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<(), String> {
    let ctx = crate::remote::core_bridge::desktop_ctx(&app, &state);
    ridge_core::commands::settings::set_user_default_cwd(&ctx, path)
        .map_err(|e| e.to_command_string())
}
