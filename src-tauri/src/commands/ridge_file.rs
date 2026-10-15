//! OS launch hints and a bounded commit request to the shared Runtime.
use serde::Serialize;
use tauri::State;
use crate::state::AppState;

pub fn save_restore_set() {
    if let Err(error) = super::runtime::workspace_file::<Vec<String>>(
        serde_json::json!({"operation":"save-restore"}),
    ) { tracing::warn!(%error,"Runtime workspace restore-set commit failed"); }
}

#[derive(Debug, Serialize)]
pub struct StartupContext {
    pub cwd: String,
    pub wind_file_in_cwd: Option<String>,
    /// A taskbar recent-workspace activation.  This wins over cwd/restore-set
    /// selection and is consumed once by the newly created WebView.
    pub workspace_file_arg: Option<String>,
    /// "cli" — process inherited a real working dir from a terminal.
    /// "menu" — process current_dir equals ridge.exe parent (双击启动).
    /// Frontend uses this to gate the restore-set logic: cli launch should not
    /// auto-open saved workspaces (user signalled intent via cwd).
    pub kind: String,
}

#[tauri::command]
pub fn get_startup_context(state: State<'_, AppState>) -> Result<StartupContext, String> {
    let cwd = state
        .startup_cli_cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "unable to determine startup cwd".to_string())?;
    let workspace_file_arg = crate::taskbar::take_pending_workspace_path().or_else(|| {
        let args = std::env::args().collect::<Vec<_>>();
        crate::taskbar::workspace_path_from_args(&args)
    });
    let kind = match state.startup_cwd_kind {
        crate::utils::cwd::StartupCwdKind::Cli => "cli",
        crate::utils::cwd::StartupCwdKind::Menu => "menu",
    }
    .to_string();
    Ok(StartupContext {
        cwd: cwd.to_string_lossy().to_string(),
        wind_file_in_cwd: None,
        workspace_file_arg,
        kind,
    })
}
