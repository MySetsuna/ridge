mod commands;
mod db;
mod engine;
mod fs;
mod state;
mod teammate;
mod types;
mod utils;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Emitter;
use tokio::sync::mpsc;
use crate::commands::{git, pane, project, terminal, workspace};
use crate::db::ProjectStore;
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (event_tx, mut event_rx) = mpsc::channel::<GlobalEvent>(256);

    // Initialize project store
    let app_data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wind");
    std::fs::create_dir_all(&app_data_dir).ok();
    let db_path = app_data_dir.join("projects.db");
    let project_store = ProjectStore::new(&db_path)
        .map_err(|e| eprintln!("Failed to initialize project store: {}", e))
        .ok();

    let mut app_state = AppState::new(event_tx);
    app_state.project_store = project_store.map(Arc::new);
    let teammate_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_clipboard_manager::init())
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
                        GlobalEvent::PaneCwdChanged {
                            workspace_id,
                            pane_id,
                            cwd,
                        } => {
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-cwd-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "cwd": cwd }),
                            );
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            git::get_git_graph,
            git::get_git_diff,
            git::set_pane_workdir,
            git::is_git_repo,
        git::get_git_info_with_cwd,
            pane::close_pane,
            pane::dock_pane,
            pane::get_pane_layout,
            pane::set_split_ratios_at_path,
            pane::set_split_ratios_batch,
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
            workspace::close_workspace,
            workspace::reorder_workspaces,
            workspace::rename_workspace,
            // Workspace history commands
            workspace::list_workspace_history,
            workspace::save_workspace,
            workspace::delete_workspace_history,
            workspace::restore_workspace,
            workspace::toggle_pin_workspace_history,
            workspace::rename_workspace_history,
            // Frontend-compatible aliases
            workspace::list_saved_workspaces,
            workspace::delete_saved_workspace,
            workspace::rename_saved_workspace,
            // Project management commands
            project::open_project,
            project::get_recent_projects,
            project::remove_project,
            project::get_file_tree,
            project::get_directory_children,
            project::text_search,
            project::filename_search,
            project::replace_in_files,
            project::read_file,
            project::get_current_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}