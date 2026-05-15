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

use tauri::{Emitter, Manager, WindowEvent};
use tokio::sync::mpsc;
use crate::commands::{fs_watch, git, pane, process, project, settings, terminal, watch, ridge_file, workspace};
use crate::db::ProjectStore;
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志 + panic hook 尽早装好，后续任何线程 panic 都会落盘到
    // `<LOCALAPPDATA>\ridge\logs\crash-YYYY-MM-DD.log`，便于事故溯源。
    let app_data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge");
    std::fs::create_dir_all(&app_data_dir).ok();
    utils::logging::init_once(&app_data_dir);

    // 事件通道容量从 256 提到 1024，减少 `cat` 大文件等高吞吐场景下
    // `event_tx.send().await` 被 backpressure 阻塞的概率。
    let (event_tx, mut event_rx) = mpsc::channel::<GlobalEvent>(1024);

    let db_path = app_data_dir.join("projects.db");
    let project_store = ProjectStore::new(&db_path)
        .map_err(|e| tracing::error!(target: "ridge::init", error = %e, "project store init failed"))
        .ok();

    let mut app_state = AppState::new(event_tx);
    app_state.project_store = project_store.map(Arc::new);
    let teammate_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_dialog::init())
        // §4 关闭即将退出 → 同步把当前所有已保存（`associated_file_path != None`）
        // 工作区路径写到 `restore_workspaces.json`，下次非 cli 启动时由前端
        // `get_restore_set` 取回并自动 reopen。这里必须同步：spawn 异步任务在
        // 进程退出前可能跑不完。
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                ridge_file::save_restore_set(app, &state);
            }
        })
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

            // Show window after initialization
            let window = app.get_webview_window("main").unwrap();
            window.show()?;

            tauri::async_runtime::spawn(async move {
                use std::collections::HashMap;
                // Adaptive coalesce window. A fixed 4ms window was fine for
                // bulk output but added pure latency to keyboard echo (BUG-4).
                // The window now scales with the previous flush's byte count:
                //   < 256 bytes  → 0ms  (echo path: dispatch immediately)
                //   < 4096 bytes → 2ms  (medium activity)
                //   ≥ 4096 bytes → 8ms  (bulk: amortise serialise overhead)
                const COALESCE_WINDOW_FAST_MS: u64 = 0;
                const COALESCE_WINDOW_MED_MS: u64 = 2;
                const COALESCE_WINDOW_SLOW_MS: u64 = 8;
                const COALESCE_MAX_BYTES: usize = 64 * 1024;
                let coalesce_window_for = |last_bytes: usize| -> u64 {
                    if last_bytes < 256 {
                        COALESCE_WINDOW_FAST_MS
                    } else if last_bytes < 4096 {
                        COALESCE_WINDOW_MED_MS
                    } else {
                        COALESCE_WINDOW_SLOW_MS
                    }
                };
                let mut pending_output: HashMap<(uuid::Uuid, uuid::Uuid), String> = HashMap::new();
                // Tracks the size of the most recent flush so the window can
                // adapt. Initialised to 0 so the first iteration uses the
                // fast window (typical: prompt redraw on shell start is small).
                let mut last_flush_bytes: usize = 0;

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
                            std::time::Duration::from_millis(coalesce_window_for(last_flush_bytes)),
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
                        Some(GlobalEvent::PaneTitleChanged {
                            workspace_id,
                            pane_id,
                            title,
                        }) => {
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-title-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "title": title }),
                            );
                        }
                        Some(GlobalEvent::PanePromptDetected {
                            workspace_id,
                            pane_id,
                        }) => {
                            // Fire-and-forget IPC. Frontend Pane.svelte listens on
                            // `pane-prompt-{ws}-{pane}` and uses it as the fast
                            // path for diff refresh (BUG-1 follow-up). Empty
                            // payload — the URL identifies the pane fully and
                            // there's no per-prompt state to convey.
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-prompt-{workspace_id}-{label}"),
                                serde_json::json!({}),
                            );
                        }
                        None => {
                            // timeout — flush all pending per-pane buffers.
                            if !pending_output.is_empty() {
                                let mut flushed_bytes: usize = 0;
                                let drained: Vec<((uuid::Uuid, uuid::Uuid), String)> =
                                    pending_output.drain().collect();
                                for ((ws, pane), payload) in drained {
                                    flushed_bytes += payload.len();
                                    let label = pane.to_string();
                                    let _ = handle.emit(
                                        &format!("pty-output-{ws}-{label}"),
                                        serde_json::json!({ "data": payload }),
                                    );
                                }
                                // Update window for the NEXT iteration based
                                // on this flush's total bytes. Bulk flushes
                                // → larger window; small echo flushes →
                                // 0ms window (immediate dispatch).
                                last_flush_bytes = flushed_bytes;
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
            git::get_git_commits_paginated,
            git::find_git_repo_root,
            git::find_git_repos_below,
            git::get_scm_status,
            git::git_stage,
            git::git_unstage,
            git::git_discard,
            git::git_clean_untracked,
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
            git::git_get_commit_files,
            git::git_get_file_versions_at_commit,
            git::git_create_tag,
            git::git_reset,
            git::git_cherry_pick,
            git::git_revert,
            git::git_op_in_progress,
            git::git_cherry_pick_abort,
            git::git_revert_abort,
            pane::close_pane,
            pane::dock_pane,
            pane::get_pane_layout,
            pane::get_pane_layout_for,
            pane::set_split_ratios_at_path,
            pane::set_split_ratios_batch,
            pane::split_pane,
            pane::toggle_mode,
            pane::register_teammate_agent,
            pane::release_teammate_agent,
            terminal::create_pane,
            terminal::activate_pane_pty,
            terminal::get_teammate_metrics,
            terminal::detect_available_shells,
            terminal::write_to_pty,
            terminal::resize_pane,
            terminal::kill_pane,
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
            project::read_opencode_history,
            process::get_pane_foreground_process,
            process::get_pane_cwd,
            // .ridge file commands
            ridge_file::save_workspace_to_file,
            ridge_file::open_workspace_from_file,
            ridge_file::delete_workspace_file,
            ridge_file::get_workspace_save_info,
            ridge_file::list_workspace_save_info,
            ridge_file::get_last_opened_workspace_path,
            ridge_file::get_startup_context,
            ridge_file::clear_last_opened_workspace_path,
            ridge_file::get_default_workspace_save_dir,
            ridge_file::browse_directory,
            ridge_file::list_recent_workspaces,
            ridge_file::clear_recent_workspaces,
            ridge_file::get_restore_set,
            ridge_file::list_saved_workspace_files,
            settings::set_user_default_cwd,
            watch::start_watching_repos,
            fs_watch::start_watching_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}