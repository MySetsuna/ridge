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
use crate::commands::{git, pane, process, project, terminal, watch, wind_file, workspace};
use crate::db::ProjectStore;
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志 + panic hook 尽早装好，后续任何线程 panic 都会落盘到
    // `<LOCALAPPDATA>\wind\logs\crash-YYYY-MM-DD.log`，便于事故溯源。
    let app_data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wind");
    std::fs::create_dir_all(&app_data_dir).ok();
    utils::logging::init_once(&app_data_dir);

    // 事件通道容量从 256 提到 1024，减少 `cat` 大文件等高吞吐场景下
    // `event_tx.send().await` 被 backpressure 阻塞的概率。
    let (event_tx, mut event_rx) = mpsc::channel::<GlobalEvent>(1024);

    let db_path = app_data_dir.join("projects.db");
    let project_store = ProjectStore::new(&db_path)
        .map_err(|e| tracing::error!(target: "wind::init", error = %e, "project store init failed"))
        .ok();

    let mut app_state = AppState::new(event_tx);
    app_state.project_store = project_store.map(Arc::new);
    let teammate_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_dialog::init())
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
                use std::collections::HashMap;
                // 合批窗口：同一 pane 的连续 PtyOutput 在 COALESCE_WINDOW_MS 内合并为一次 emit，
                // 显著降低高吞吐（`cat huge.log`）场景下的 IPC 次数与前端渲染压力。
                const COALESCE_WINDOW_MS: u64 = 4;
                const COALESCE_MAX_BYTES: usize = 64 * 1024;
                let mut pending_output: HashMap<(uuid::Uuid, uuid::Uuid), String> = HashMap::new();

                // 事件循环：
                //   - 无积压 PtyOutput 时，无限等待下一条事件；
                //   - 有积压时，最多等一个合批窗口后强制 flush；
                //   - 任何 emit 失败只记录不中断。
                enum Tick {
                    Event(GlobalEvent),
                    Flush,
                    Closed,
                }
                loop {
                    let tick: Tick = if pending_output.is_empty() {
                        match event_rx.recv().await {
                            Some(ev) => Tick::Event(ev),
                            None => Tick::Closed,
                        }
                    } else {
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(COALESCE_WINDOW_MS),
                            event_rx.recv(),
                        )
                        .await
                        {
                            Ok(Some(ev)) => Tick::Event(ev),
                            Ok(None) => Tick::Closed,
                            Err(_) => Tick::Flush,
                        }
                    };

                    if matches!(tick, Tick::Closed) {
                        for ((ws, pane), data) in pending_output.drain() {
                            let label = pane.to_string();
                            let _ = handle.emit(
                                &format!("pty-output-{ws}-{label}"),
                                serde_json::json!({ "data": data }),
                            );
                        }
                        break;
                    }

                    let ev = match tick {
                        Tick::Event(ev) => Some(ev),
                        Tick::Flush => None,
                        Tick::Closed => unreachable!(),
                    };

                    match ev {
                        Some(GlobalEvent::PtyOutput {
                            workspace_id,
                            pane_id,
                            data,
                        }) => {
                            let entry = pending_output
                                .entry((workspace_id, pane_id))
                                .or_insert_with(String::new);
                            entry.push_str(&data);
                            // 单个 pane 的缓冲超过阈值时立刻 flush，避免一次大块长期滞留。
                            if entry.len() >= COALESCE_MAX_BYTES {
                                let payload = std::mem::take(entry);
                                pending_output.remove(&(workspace_id, pane_id));
                                let label = pane_id.to_string();
                                let _ = handle.emit(
                                    &format!("pty-output-{workspace_id}-{label}"),
                                    serde_json::json!({ "data": payload }),
                                );
                            }
                        }
                        Some(GlobalEvent::PaneClosed {
                            workspace_id,
                            pane_id,
                        }) => {
                            // pane 关闭前强制 flush 它自己的 buffer，避免尾部输出丢失。
                            if let Some(payload) = pending_output.remove(&(workspace_id, pane_id)) {
                                let label = pane_id.to_string();
                                let _ = handle.emit(
                                    &format!("pty-output-{workspace_id}-{label}"),
                                    serde_json::json!({ "data": payload }),
                                );
                            }
                            let _ = handle.emit(
                                "pane-pty-closed",
                                serde_json::json!({
                                    "workspaceId": workspace_id.to_string(),
                                    "paneId": pane_id.to_string(),
                                }),
                            );
                        }
                        Some(GlobalEvent::PaneModeChanged {
                            workspace_id,
                            pane_id,
                            mode,
                        }) => {
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
                        Some(GlobalEvent::PaneCwdChanged {
                            workspace_id,
                            pane_id,
                            cwd,
                        }) => {
                            // cwd 事件早于 pty 缓冲 flush，否则资源管理器/源代码管理
                            // 可能看到旧 cwd 下的输出被当作新 cwd 的内容。
                            if let Some(payload) = pending_output.remove(&(workspace_id, pane_id)) {
                                let label = pane_id.to_string();
                                let _ = handle.emit(
                                    &format!("pty-output-{workspace_id}-{label}"),
                                    serde_json::json!({ "data": payload }),
                                );
                            }
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-cwd-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "cwd": cwd }),
                            );
                        }
                        None => {
                            // timeout — flush all pending per-pane buffers.
                            if !pending_output.is_empty() {
                                let drained: Vec<((uuid::Uuid, uuid::Uuid), String)> =
                                    pending_output.drain().collect();
                                for ((ws, pane), payload) in drained {
                                    let label = pane.to_string();
                                    let _ = handle.emit(
                                        &format!("pty-output-{ws}-{label}"),
                                        serde_json::json!({ "data": payload }),
                                    );
                                }
                            }
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
            git::find_git_repo_root,
            git::find_git_repos_below,
            git::get_scm_status,
            git::git_stage,
            git::git_unstage,
            git::git_discard,
            git::git_commit,
            git::git_list_branches,
            git::git_checkout,
            git::git_fetch,
            git::git_pull,
            git::git_push,
            git::git_sync,
            git::git_diff_file,
            git::git_diff_summary,
            git::git_get_file_versions,
            git::git_cherry_pick,
            git::git_revert,
            git::git_op_in_progress,
            git::git_cherry_pick_abort,
            git::git_revert_abort,
            pane::close_pane,
            pane::dock_pane,
            pane::get_pane_layout,
            pane::set_split_ratios_at_path,
            pane::set_split_ratios_batch,
            pane::split_pane,
            pane::toggle_mode,
            pane::register_teammate_agent,
            pane::release_teammate_agent,
            terminal::create_pane,
            terminal::write_to_pty,
            terminal::resize_pane,
            terminal::kill_pane,
            terminal::get_pane_scrollback,
            terminal::get_pane_scrollback_tail,
            terminal::get_pane_scrollback_before,
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
            project::text_search_diagnostics,
            project::filename_search,
            project::replace_in_files,
            project::read_file,
            project::read_file_for_editor,
            project::write_file,
            project::get_current_project,
            project::rename_path,
            project::delete_path,
            project::create_file,
            project::create_directory,
            project::reveal_in_file_manager,
            project::copy_path,
            project::move_path,
            project::read_claude_history,
            process::get_pane_foreground_process,
            process::get_pane_cwd,
            // .wind file commands
            wind_file::save_workspace_to_file,
            wind_file::open_workspace_from_file,
            wind_file::delete_workspace_file,
            wind_file::get_workspace_save_info,
            wind_file::list_workspace_save_info,
            wind_file::get_last_opened_workspace_path,
            wind_file::get_startup_context,
            wind_file::clear_last_opened_workspace_path,
            wind_file::get_default_workspace_save_dir,
            wind_file::browse_directory,
            wind_file::list_recent_workspaces,
            wind_file::clear_recent_workspaces,
            watch::start_watching_repos,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}