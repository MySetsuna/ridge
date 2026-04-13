mod commands;
mod engine;
mod state;
mod teammate;
mod types;
mod utils;

use tauri::Emitter;
use tokio::sync::mpsc;
use crate::commands::{git, pane, terminal, workspace};
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (event_tx, mut event_rx) = mpsc::channel::<GlobalEvent>(256);
    let app_state = AppState::new(event_tx);
    let teammate_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .setup(move |app| {
            let handle = app.handle().clone();
            let (teammate_ready_tx, teammate_ready_rx) = std::sync::mpsc::channel();
            teammate::spawn_teammate_server(
                handle.clone(),
                teammate_state.clone(),
                Some(teammate_ready_tx),
            );
            let _ = teammate_ready_rx.recv_timeout(std::time::Duration::from_secs(5));
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = event_rx.recv().await {
                    match ev {
                        GlobalEvent::PtyOutput {
                            workspace_id,
                            pane_id,
                            data,
                        } => {
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pty-output-{workspace_id}-{label}"),
                                serde_json::json!({ "data": data }),
                            );
                        }
                        GlobalEvent::PaneClosed {
                            workspace_id,
                            pane_id,
                        } => {
                            let _ = handle.emit(
                                "pane-pty-closed",
                                serde_json::json!({
                                    "workspaceId": workspace_id.to_string(),
                                    "paneId": pane_id.to_string(),
                                }),
                            );
                        }
                        GlobalEvent::PaneModeChanged {
                            workspace_id,
                            pane_id,
                            mode,
                        } => {
                            let mode_str = match &mode {
                                PaneMode::Terminal => "Terminal",
                                PaneMode::Editor { .. } => "Editor",
                            };
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-mode-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "mode": mode_str }),
                            );
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            git::get_git_graph,
            pane::close_pane,
            pane::get_pane_layout,
            pane::split_pane,
            pane::toggle_mode,
            terminal::create_pane,
            terminal::write_to_pty,
            terminal::resize_pane,
            terminal::kill_pane,
            workspace::create_workspace,
            workspace::get_active_workspace_id,
            workspace::list_workspaces,
            workspace::switch_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
