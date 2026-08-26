mod commands;
mod db;
mod deep_root;
mod engine;
mod fs;
mod hosts;
mod kernel_lifecycle;
mod lsp;
pub mod reconnect_policy;
/// 桌面进程与共享远控层之间的 Tauri 胶水层（`forward_event` + `ridge-core` 桥
/// + `spawn_remote_server` 启动壳）——归并自已删除的 `remote/{mod,core_bridge,server}.rs`。
mod remote_bridge;
/// 桌面 `RemoteHost` 实现（`DesktopHost` 包装 `AppState`）。
mod remote_host_impl;
mod state;
mod taskbar;
mod teammate;
mod tray;
mod types;
mod utils;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::commands::{
    clipboard_files, clipboard_image, fs_watch, git, pane, process, project, ridge_file, settings,
    terminal, terminal_font, theme, watch, workspace,
};
use crate::db::ProjectStore;
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

/// 窗口几何持久化的维度：大小 / 位置 / 最大化 / 全屏。
///
/// 刻意**不含** `VISIBLE` 与 `DECORATIONS`：
///   - 可见性由 Deep Root（hide-to-tray）/ 托盘逻辑掌控，存了会让深根隐藏后下次
///     启动以「隐藏」态恢复（窗口开不出来）。
///   - 装饰恒为关（`decorations(false)` 自绘标题栏），无需也不应被状态覆盖。
fn window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}
use tokio::sync::mpsc;

static NEXT_WINDOW_LABEL: AtomicU64 = AtomicU64::new(1);

fn is_auth_focus_launch(argv: &[String]) -> bool {
    argv.iter()
        .any(|arg| arg.to_ascii_lowercase().starts_with("ridge://auth/focus"))
}

fn build_ridge_window(
    app: &tauri::AppHandle,
    label: &str,
    restore_geometry: bool,
) -> tauri::Result<tauri::WebviewWindow> {
    let app_data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge");
    let splash_init_script = theme::build_splash_init_script(app, &app_data_dir);
    let mut builder = WebviewWindowBuilder::new(app, label.to_owned(), WebviewUrl::default());
    #[cfg(windows)]
    if let Ok(extra) = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        if !extra.trim().is_empty() {
            builder = builder.additional_browser_args(&extra);
        }
    }
    if std::env::var_os("RIDGE_E2E").is_some() {
        builder = builder.initialization_script(
            r#"Object.defineProperty(window, "__RIDGE_E2E__", { value: true, configurable: false, writable: false });"#,
        );
    }
    let window = builder
        .title("ridge")
        .inner_size(800.0, 600.0)
        .decorations(false)
        .visible(false)
        .devtools(true)
        .initialization_script(&splash_init_script)
        .build()?;
    if restore_geometry {
        if let Err(error) = window.restore_state(window_state_flags()) {
            tracing::warn!(
                target: "ridge::init",
                %error,
                "restore window state failed; using default geometry"
            );
        }
    }
    window.show()?;
    Ok(window)
}

fn open_secondary_window(app: &tauri::AppHandle) {
    let serial = NEXT_WINDOW_LABEL.fetch_add(1, Ordering::Relaxed);
    let label = format!("ridge-window-{serial}");
    if let Err(error) = build_ridge_window(app, &label, false) {
        tracing::error!(
            target: "ridge::init",
            %label,
            %error,
            "failed to create secondary window"
        );
        crate::deep_root::focus_main_window(app);
    }
}

fn register_remote_event_listeners(app: &tauri::App, handle: &tauri::AppHandle) {
    use tauri::Listener;
    for name in [
        "teammate-layout-changed",
        "teammate-active-pane-changed",
        "lsp://diagnostics",
    ] {
        let forward_handle = handle.clone();
        app.listen_any(name, move |event| {
            let payload = serde_json::from_str(event.payload()).unwrap_or(serde_json::Value::Null);
            crate::remote_bridge::forward_event(&forward_handle, name, payload);
        });
    }
}

fn start_kernel_bootstrap(app: &tauri::App) {
    let kernel_handle = app.handle().clone();
    let kernel_hosts = app.state::<crate::state::AppState>().hosts.clone();
    let kernel_stop = app.state::<crate::state::AppState>().quitting.clone();
    tauri::async_runtime::spawn(async move {
        let bootstrap = tauri::async_runtime::spawn_blocking(|| {
            let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
            let hosts = crate::hosts::kernel_host_snapshot();
            Ok::<_, String>((endpoint, hosts))
        })
        .await;
        match bootstrap {
            Ok(Ok((endpoint, host_snapshot))) => {
                tracing::info!(target: "ridge::kernel_lifecycle", pid = endpoint.pid, port = endpoint.port, "ridge-kernel ready");
                crate::commands::workspace::sync_kernel_workspace_topologies(
                    &*kernel_handle.state::<crate::state::AppState>(),
                );
                match host_snapshot {
                    Ok(records) => kernel_hosts.restore_topology(records),
                    Err(error) => {
                        tracing::warn!(target: "ridge::kernel_lifecycle", %error, "kernel host topology restore unavailable; shell will fail closed")
                    }
                }
                let exit_handle = kernel_handle.clone();
                let watcher_stop = kernel_stop.clone();
                if let Err(error) = crate::kernel_lifecycle::spawn_kernel_death_watcher(
                    endpoint,
                    move || {
                        exit_handle
                            .state::<crate::state::AppState>()
                            .quitting
                            .store(true, Ordering::Release);
                        exit_handle.exit(0);
                    },
                    move || watcher_stop.load(Ordering::Acquire),
                ) {
                    tracing::error!(target: "ridge::kernel_lifecycle", %error, "failed to spawn ridge-kernel death watcher");
                }
            }
            Ok(Err(error)) => {
                tracing::error!(target: "ridge::kernel_lifecycle", %error, "failed to start/attach ridge-kernel (shell continues; deep-root incomplete)")
            }
            Err(error) => {
                tracing::error!(target: "ridge::kernel_lifecycle", %error, "kernel bootstrap task failed (shell continues; deep-root incomplete)")
            }
        }
    });
}

fn register_deep_links(app: &tauri::App) {
    use tauri_plugin_deep_link::DeepLinkExt;
    if let Err(error) = app.deep_link().register_all() {
        tracing::warn!(target: "ridge::deep_link", %error, "deep-link scheme runtime registration failed (continuing)");
    }
    let handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        let urls: Vec<String> = event.urls().iter().map(ToString::to_string).collect();
        tracing::info!(target: "ridge::deep_link", ?urls, "deep link opened");
        crate::deep_root::focus_main_window(&handle);
    });
}

fn setup_app(
    app: &mut tauri::App,
    teammate_state: AppState,
    event_rx: mpsc::Receiver<GlobalEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(target: "ridge::init", phase = 1, "setup: storing AppHandle");
    clipboard_image::cleanup_old_temp_images(std::time::Duration::from_secs(3600));
    let handle = app.handle().clone();
    lsp::set_app_handle(handle.clone());
    let _ = teammate_state.app_handle.set(handle.clone());
    if let Ok(base) = handle.path().app_data_dir() {
        teammate::memory::init_dir(base.join("workspace-memory"));
    }
    teammate::suspend::load_all_for();
    register_remote_event_listeners(app, &handle);
    build_ridge_window(app.handle(), "main", true)?;
    start_kernel_bootstrap(app);
    if let Err(error) = crate::tray::build_tray(app) {
        tracing::error!(target: "ridge::tray", error = %error, "tray init failed");
    }
    crate::taskbar::refresh_jump_list_async(handle.clone());
    register_deep_links(app);
    spawn_event_forwarder(handle, event_rx);
    Ok(())
}

fn configure_single_instance(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    if std::env::var_os("RIDGE_DISABLE_SINGLE_INSTANCE").is_some() {
        return builder;
    }
    builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        if is_auth_focus_launch(&argv) {
            crate::deep_root::focus_main_window(app);
            return;
        }
        if let Some(path) = crate::taskbar::workspace_path_from_args(&argv) {
            crate::taskbar::enqueue_workspace_path(path);
        }
        open_secondary_window(app);
    }))
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    let app = window.app_handle();
    let state = app.state::<AppState>();
    if matches!(event, WindowEvent::Destroyed) {
        state.workspace_window_claims.release_window(window.label());
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() != "main" {
            state.workspace_window_claims.release_window(window.label());
            return;
        }
        if !state.quitting.load(std::sync::atomic::Ordering::Acquire) {
            if let Err(error) = app.save_window_state(window_state_flags()) {
                tracing::warn!(target: "ridge::init", error = %error, "save window state on hide-to-tray failed");
            }
            ridge_file::save_restore_set(&app, &state);
            api.prevent_close();
            crate::deep_root::prepare_for_hide(window);
            if let Err(error) = window.hide() {
                tracing::warn!(target: "ridge::deep_root", error = %error, "hide-to-tray on close-requested failed");
            }
            return;
        }
        if let Err(error) = app.save_window_state(window_state_flags()) {
            tracing::warn!(target: "ridge::init", error = %error, "save window state on quit failed");
        }
        crate::commands::remote::stop_remote_server(&state);
        ridge_file::save_restore_set(&app, &state);
    }
}

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
    let (event_tx, event_rx) = mpsc::channel::<GlobalEvent>(1024);

    let db_path = app_data_dir.join("projects.db");
    let project_store = ProjectStore::new(&db_path)
        .map_err(
            |e| tracing::error!(target: "ridge::init", error = %e, "project store init failed"),
        )
        .ok();

    let mut app_state = AppState::new(event_tx);
    app_state.project_store = project_store.map(Arc::new);
    // §blacklist: load the persistent remote-control blacklist (devices/IPs
    // barred from connecting) from the app data dir.
    app_state
        .remote_blacklist
        .set_path_and_load(app_data_dir.join("remote-blacklist.json"));
    let teammate_state = app_state.clone();

    let builder = configure_single_instance(tauri::Builder::default());
    // 公网登录授权（契约 §1）：single-instance 必须最先注册——浏览器唤起
    // `ridge://auth/focus` 时 Windows 会启动第二个进程，此插件把它的 argv 转交
    // 给首个实例并触发下面的回调，我们据此聚焦主窗口并广播 auth-focus 事件。
    //
    // 例外：设置 `RIDGE_DISABLE_SINGLE_INSTANCE` 时跳过注册——专供
    // `tauri:dev:cdp` 让一个带 CDP 的调试实例与已安装的正式版并存联调
    // （正式版持有 single-instance 锁；调试实例若也注册会被立即聚焦并退出）。
    // 仅该 dev 工作流设置此变量；正式构建从不设置，启动行为完全不变。
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        // Deep Root Mode（§8.1）：进入深根时发原生系统通知（NotificationExt）。
        .plugin(tauri_plugin_notification::init())
        // 记住上次窗口几何（大小/位置/最大化/全屏）。插件在 `RunEvent::Exit`（彻底退出）
        // 自动存盘，并持续缓存 Moved/Resized 事件的几何；恢复则由 setup 里 show() 之前的
        // `window.restore_state(...)` 显式执行（避免先以默认 800×600 绘制再跳变）。
        // `skip_initial_state("main")`：本窗口由代码运行时创建（非 tauri.conf 声明），手动
        // 恢复已覆盖，跳过插件自动恢复以免重复/迟到 restore 造成可见跳变。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .skip_initial_state("main")
                .build(),
        )
        // §4 关闭即将退出 → 同步把当前所有已保存（`associated_file_path != None`）
        // 工作区路径写到 `restore_workspaces.json`，下次非 cli 启动时由前端
        // `get_restore_set` 取回并自动 reopen。这里必须同步：spawn 异步任务在
        // 进程退出前可能跑不完。
        .on_window_event(handle_window_event)
        .manage(app_state)
        .setup(move |app| setup_app(app, teammate_state, event_rx))
        .invoke_handler(tauri::generate_handler![
            git::get_git_graph,
            git::get_git_diff,
            git::set_pane_workdir,
            git::get_git_guard_stats,
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
            git::git_merge_branch,
            git::git_delete_branch,
            git::git_rename_branch,
            git::git_push_branch,
            git::git_rebase,
            git::git_delete_tag,
            git::git_push_tag,
            git::git_stash_list,
            git::git_stash_push,
            git::git_stash_apply,
            git::git_stash_pop,
            git::git_stash_drop,
            git::git_stash_branch,
            git::git_fetch,
            git::git_pull,
            git::git_push,
            git::git_sync,
            git::git_diff_file,
            git::git_blame,
            git::git_file_log,
            lsp::lsp_did_open,
            lsp::lsp_did_change,
            lsp::lsp_definition,
            lsp::lsp_hover,
            lsp::lsp_references,
            git::git_diff_summary,
            git::git_get_file_versions,
            git::git_get_commit_files,
            git::git_get_file_versions_at_commit,
            git::git_get_file_versions_between,
            git::git_compare_commits,
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
            pane::get_window_pane_layout,
            pane::get_pane_layout_for,
            pane::set_split_ratios_at_path,
            pane::set_window_split_ratios_at_path,
            pane::set_split_ratios_batch,
            pane::set_window_split_ratios_batch,
            pane::split_pane,
            pane::resume_agent_session,
            pane::toggle_mode,
            pane::register_teammate_agent,
            pane::release_teammate_agent,
            terminal::create_pane,
            terminal_font::load_terminal_font_faces,
            terminal_font::read_terminal_font_face_chunk,
            terminal::activate_pane_pty,
            terminal::get_teammate_metrics,
            terminal::change_pane_shell,
            terminal::launch_agent_session,
            terminal::detect_available_shells,
            terminal::get_shell_history,
            terminal::write_to_pty,
            clipboard_image::read_clipboard_image_to_temp,
            clipboard_image::save_clipboard_image_to_temp,
            clipboard_image::resolve_pasted_image_path,
            clipboard_files::read_clipboard_file_paths,
            clipboard_files::write_clipboard_file_paths,
            clipboard_files::read_clipboard_sequence,
            terminal::resize_pane,
            terminal::reattach_kernel_ptys,
            terminal::clear_pane_terminal,
            terminal::set_pane_delta_mode,
            terminal::register_pane_delta_channel,
            terminal::take_pane_delta_frame,
            terminal::kill_pane,
            terminal::get_pane_scrollback_tail,
            terminal::get_pane_scrollback_before,
            terminal::get_pane_resync_frame,
            terminal::list_native_sessions,
            terminal::summon_native_session,
            terminal::new_headless_session,
            terminal::terminate_native_session,
            // 「主机 / Hosts」远端主机登记（P3/P4 基础层，桌面本地命令，不入远程白名单）。
            hosts::host_list_snapshot,
            hosts::register_frontend_host,
            hosts::connect_host,
            hosts::disconnect_host,
            hosts::forget_host,
            hosts::attach_host_session,
            hosts::detach_host_session,
            hosts::list_host_sessions,
            hosts::inject_host_output,
            hosts::get_outbound_stats,
            hosts::get_live_backpressure,
            hosts::pump_host_output,
            hosts::bind_mock_outbound_and_list,
            hosts::step_host_reconnect,
            hosts::cancel_host_reconnect,
            hosts::history_commands::get_foreign_history_tail,
            hosts::history_commands::append_foreign_history,
            hosts::history_commands::foreign_history_pull_budget,
            hosts::history_commands::set_foreign_history_cap,
            workspace::create_workspace,
            workspace::get_active_workspace_id,
            workspace::get_window_active_workspace_id,
            workspace::list_workspaces,
            workspace::get_domain_convergence_report,
            workspace::acquire_window_workspace,
            workspace::claim_workspace_window,
            workspace::create_workspace_for_window,
            workspace::switch_workspace,
            workspace::switch_window_workspace,
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
            project::path_exists,
            project::read_claude_history,
            project::read_agent_recent_replies,
            project::list_agent_profiles,
            project::save_agent_profile_overrides,
            project::plan_agent_resume,
            project::read_opencode_history,
            project::get_git_changed_files,
            process::get_pane_foreground_process,
            process::get_pane_cwd,
            // .ridge file commands
            ridge_file::save_workspace_to_file,
            ridge_file::open_workspace_from_file,
            ridge_file::delete_workspace_file,
            ridge_file::delete_saved_workspace_file,
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
            theme::get_theme_data,
            theme::set_active_theme,
            theme::get_active_theme_entry,
            theme::save_user_theme,
            theme::delete_user_theme,
            theme::save_theme_bg_image,
            theme::save_theme_bg_image_from_path,
            theme::get_theme_assets_dir,
            watch::start_watching_repos,
            fs_watch::start_watching_paths,
            commands::remote::get_remote_info,
            commands::remote::remote_reap_orphans,
            commands::remote::verify_remote_totp,
            commands::remote::get_device_identity_pub,
            commands::remote::sign_device_identity,
            commands::remote::verify_remote_totp_bind,
            commands::remote::remote_reset_totp,
            commands::remote::remote_set_totp_identity,
            commands::remote::totp_trust_check,
            commands::remote::totp_trust_record,
            commands::remote::totp_trust_revoke_all,
            commands::remote::set_remote_enabled,
            commands::remote::get_remote_enabled,
            commands::remote::list_remote_sessions,
            commands::remote::disconnect_session,
            commands::remote::add_to_blacklist,
            commands::remote::list_blacklist,
            commands::remote::remove_from_blacklist,
            // B2（D-GM-11）cloud pane 裸字节流（host-local sink，非 controller 直调）
            commands::cloud_pane::subscribe_pane_raw,
            commands::cloud_pane::unsubscribe_pane_raw,
            commands::cloud_pane::resync_pane_raw,
            commands::cloud_pane::replay_pane_scrollback_raw,
            // 桌面 cloud HTTP 代理（绕过 WebView 跨域 CORS，见 cloud_http.rs）
            commands::cloud_http::cloud_http,
            // Domain Zero 端侧多智能体协同（teammate）：D1 拓扑快照 + D2 HITL 网关/风险分级
            commands::teammate::get_teammate_topology,
            commands::teammate::send_agent_message,
            commands::teammate::publish_pty_runtime_snapshot,
            commands::teammate::get_pty_runtime_identity,
            commands::teammate::list_hitl_pending,
            commands::teammate::resolve_hitl_remote,
            commands::teammate::list_hitl_pending_local,
            commands::teammate::list_execution_rejections_local,
            commands::teammate::dismiss_execution_rejection,
            // M1 切片二：裁决审计历史（仅桌面 IPC）
            commands::teammate::list_hitl_decisions,
            // G1 阶段一软暂停 + 可选 OS 冻结（仅桌面本机 IPC，不入 REMOTE_ALLOWLIST）
            commands::teammate::suspend_agent,
            commands::teammate::resume_agent,
            commands::teammate::get_orchestration_health,
            commands::teammate::get_pending_hitl_count,
            commands::teammate::list_hitl_audit_remote,
            commands::teammate::scan_workspace_context_files,
            // M1 切片三 + V-DISC + V-G1-RB
            commands::teammate::get_workspace_memory,
            commands::teammate::set_workspace_memory,
            commands::teammate::set_teammate_groups,
            commands::teammate::checkpoint_workspace_rollback,
            commands::teammate::rollback_workspace,
            commands::teammate::resolve_hitl_request,
            commands::teammate::set_hitl_enabled,
            commands::teammate::classify_command_risk,
            // Domain D3 文件并发写锁（前端冲突仲裁视图用）
            teammate::locks::acquire_write_lock,
            teammate::locks::release_write_lock,
            teammate::locks::write_lock_holder,
            // Deep Root Mode（§8.1）
            deep_root::enter_deep_root_mode,
            deep_root::restore_from_deep_root,
            deep_root::set_cloud_remote_active,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

const COALESCE_MAX_BYTES: usize = 64 * 1024;
// Agent activity only nudges the control plane; terminal bytes stay lossless
// and immediate. Four updates per second are enough for status/highlight UI
// without competing with the terminal render lane during agent output bursts.
const OUTPUT_ACTIVITY_INTERVAL: Duration = Duration::from_millis(250);

fn coalesce_window_for(_last_bytes: usize) -> u64 {
    // Never insert a timer gap between PTY packets. The forwarder still
    // drains packets already ready in the channel, while interactive output
    // (notably repeated Ctrl+C) reaches the renderer without a periodic
    // 2/8ms cadence.
    0
}

type PendingOutput = HashMap<(uuid::Uuid, uuid::Uuid), String>;
type OutputActivityAt = HashMap<(uuid::Uuid, uuid::Uuid), Instant>;

fn output_activity_due(last: Option<Instant>, now: Instant) -> bool {
    last.map_or(true, |at| {
        now.duration_since(at) >= OUTPUT_ACTIVITY_INTERVAL
    })
}

fn emit_pane_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: String,
) {
    let label = pane_id.to_string();
    // Terminal bytes are the foreground lane. Emit them before the optional
    // Agent control-plane hint so status refresh listeners cannot get ahead
    // of the visible shell/Codex output.
    let _ = handle.emit(
        &format!("pty-output-{workspace_id}-{label}"),
        serde_json::json!({ "data": data }),
    );

    emit_pane_activity(handle, activity_at, workspace_id, pane_id);
}

fn emit_pane_activity(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
) {
    // Activity is a low-priority control-plane hint, not terminal data. Keep
    // it bounded while the PTY emits a burst; the terminal frame remains
    // lossless and immediate.
    let label = pane_id.to_string();
    let key = (workspace_id, pane_id);
    let state = handle.state::<AppState>();
    let is_teammate_pane = state
        .workspaces
        .read()
        .get(&workspace_id)
        .is_some_and(|workspace| workspace.teammate_pane_states.contains_key(&pane_id));
    if is_teammate_pane {
        let now = Instant::now();
        if output_activity_due(activity_at.get(&key).copied(), now) {
            activity_at.insert(key, now);
            let _ = handle.emit(
                "pane-output-activity",
                serde_json::json!({
                    "workspaceId": workspace_id.to_string(), "paneId": label,
                }),
            );
        }
    }
}

fn emit_pane_delta(
    handle: &tauri::AppHandle,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    frame: Vec<u8>,
) {
    let state = handle.state::<AppState>();
    if let Some(sender) = state.get_pane_delta_channel(workspace_id, pane_id) {
        sender(frame);
    } else {
        let label = pane_id.to_string();
        let _ = handle.emit(&format!("pty-delta-{workspace_id}-{label}"), frame);
    }
}

fn flush_pane_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
) {
    if let Some(data) = pending.remove(&(workspace_id, pane_id)) {
        emit_pane_output(handle, activity_at, workspace_id, pane_id, data);
    }
}

fn flush_pending_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
) -> usize {
    let drained: Vec<_> = pending.drain().collect();
    let bytes = drained.iter().map(|(_, data)| data.len()).sum();
    for ((workspace_id, pane_id), data) in drained {
        emit_pane_output(handle, activity_at, workspace_id, pane_id, data);
    }
    bytes
}

fn forward_raw_pty_output(
    handle: &tauri::AppHandle,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: &str,
) {
    handle
        .state::<AppState>()
        .forward_remote_pty_bytes(workspace_id, pane_id, data);
}

fn handle_pty_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: String,
) {
    forward_raw_pty_output(handle, workspace_id, pane_id, &data);
    // The reader thread always keeps the native parser warm. A raw event that
    // was queued just before delta enable is therefore already represented by
    // the full reframe sent during the handoff; do not parse it a second time.
    let delta_enabled = handle
        .state::<AppState>()
        .workspaces
        .read()
        .get(&workspace_id)
        .and_then(|workspace| workspace.terminals.get(&pane_id))
        .is_some_and(|terminal| terminal.delta_mode.load(Ordering::Acquire));
    if delta_enabled {
        return;
    }
    let entry = pending.entry((workspace_id, pane_id)).or_default();
    entry.push_str(&data);
    if entry.len() >= COALESCE_MAX_BYTES {
        let payload = std::mem::take(entry);
        pending.remove(&(workspace_id, pane_id));
        emit_pane_output(handle, activity_at, workspace_id, pane_id, payload);
    }
}

fn handle_pty_delta_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: String,
    frame: Vec<u8>,
) {
    // Remote subscribers still receive the lossless raw stream. The local
    // desktop path receives only the already-parsed binary frame.
    forward_raw_pty_output(handle, workspace_id, pane_id, &data);
    if !frame.is_empty() {
        emit_pane_delta(handle, workspace_id, pane_id, frame);
    }
    emit_pane_activity(handle, activity_at, workspace_id, pane_id);
}

fn handle_cwd_changed(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    cwd: String,
) {
    flush_pane_output(handle, activity_at, pending, workspace_id, pane_id);
    let label = pane_id.to_string();
    let state = handle.state::<AppState>();
    if state.remote_enabled.load(Ordering::Relaxed) {
        state.broadcast_remote_event(
            workspace_id,
            pane_id,
            crate::types::RemotePtyEvent::Metadata {
                workspace_id,
                pane_id,
                title: None,
                cwd: Some(cwd.clone()),
            },
        );
    }
    let _ = handle.emit(
        "pane-meta-changed",
        serde_json::json!({
            "workspaceId": workspace_id.to_string(), "paneId": label, "cwd": &cwd,
        }),
    );
    let _ = handle.emit(
        &format!("pane-cwd-changed-{workspace_id}-{label}"),
        serde_json::json!({ "cwd": cwd }),
    );
}

fn handle_title_changed(
    handle: &tauri::AppHandle,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    title: String,
) {
    let label = pane_id.to_string();
    let state = handle.state::<AppState>();
    if state.remote_enabled.load(Ordering::Relaxed) {
        state.broadcast_remote_event(
            workspace_id,
            pane_id,
            crate::types::RemotePtyEvent::Metadata {
                workspace_id,
                pane_id,
                title: Some(title.clone()),
                cwd: None,
            },
        );
    }
    let _ = handle.emit(
        "pane-meta-changed",
        serde_json::json!({
            "workspaceId": workspace_id.to_string(), "paneId": label, "title": &title,
        }),
    );
    let _ = handle.emit(
        &format!("pane-title-changed-{workspace_id}-{label}"),
        serde_json::json!({ "title": title }),
    );
}

fn handle_global_event(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    event: GlobalEvent,
) {
    match event {
        GlobalEvent::PtyOutput {
            workspace_id,
            pane_id,
            data,
        } => {
            handle_pty_output(handle, activity_at, pending, workspace_id, pane_id, data);
        }
        GlobalEvent::PtyDeltaOutput {
            workspace_id,
            pane_id,
            data,
            frame,
        } => {
            handle_pty_delta_output(handle, activity_at, workspace_id, pane_id, data, frame);
        }
        GlobalEvent::PaneClosed {
            workspace_id,
            pane_id,
        } => {
            flush_pane_output(handle, activity_at, pending, workspace_id, pane_id);
            activity_at.remove(&(workspace_id, pane_id));
            let _ = handle.emit(
                "pane-pty-closed",
                serde_json::json!({
                    "workspaceId": workspace_id.to_string(), "paneId": pane_id.to_string(),
                }),
            );
        }
        GlobalEvent::PaneModeChanged {
            workspace_id,
            pane_id,
            mode,
        } => {
            let mode = match mode {
                PaneMode::Terminal => "Terminal",
                PaneMode::Editor { .. } => "Editor",
            };
            let _ = handle.emit(
                &format!("pane-mode-changed-{workspace_id}-{pane_id}"),
                serde_json::json!({ "mode": mode }),
            );
        }
        GlobalEvent::PaneCwdChanged {
            workspace_id,
            pane_id,
            cwd,
        } => handle_cwd_changed(handle, activity_at, pending, workspace_id, pane_id, cwd),
        GlobalEvent::PaneTitleChanged {
            workspace_id,
            pane_id,
            title,
        } => handle_title_changed(handle, workspace_id, pane_id, title),
        GlobalEvent::PanePromptDetected {
            workspace_id,
            pane_id,
        } => {
            let _ = handle.emit(
                &format!("pane-prompt-{workspace_id}-{pane_id}"),
                serde_json::json!({}),
            );
        }
        GlobalEvent::PaneTreeChanged { workspace_id } => {
            let _ = handle.emit(
                "pane-tree-changed",
                serde_json::json!({ "workspaceId": workspace_id.to_string() }),
            );
        }
        GlobalEvent::WorkspaceListChanged => {
            let _ = handle.emit("workspace-list-changed", serde_json::json!({}));
        }
    }
}

enum ForwardTick {
    Event(GlobalEvent),
    Flush,
    Closed,
}

async fn next_forward_tick(
    event_rx: &mut mpsc::Receiver<GlobalEvent>,
    pending: &PendingOutput,
    last_flush_bytes: usize,
) -> ForwardTick {
    if pending.is_empty() {
        return event_rx
            .recv()
            .await
            .map_or(ForwardTick::Closed, ForwardTick::Event);
    }
    if coalesce_window_for(last_flush_bytes) == 0 {
        return match event_rx.try_recv() {
            Ok(event) => ForwardTick::Event(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => ForwardTick::Flush,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => ForwardTick::Closed,
        };
    }
    match tokio::time::timeout(
        std::time::Duration::from_millis(coalesce_window_for(last_flush_bytes)),
        event_rx.recv(),
    )
    .await
    {
        Ok(Some(event)) => ForwardTick::Event(event),
        Ok(None) => ForwardTick::Closed,
        Err(_) => ForwardTick::Flush,
    }
}

async fn run_event_forwarder(handle: tauri::AppHandle, mut event_rx: mpsc::Receiver<GlobalEvent>) {
    let mut pending = PendingOutput::new();
    let mut activity_at = OutputActivityAt::new();
    let mut last_flush_bytes = 0;
    loop {
        match next_forward_tick(&mut event_rx, &pending, last_flush_bytes).await {
            ForwardTick::Event(event) => {
                handle_global_event(&handle, &mut activity_at, &mut pending, event)
            }
            ForwardTick::Flush => {
                last_flush_bytes = flush_pending_output(&handle, &mut activity_at, &mut pending)
            }
            ForwardTick::Closed => {
                flush_pending_output(&handle, &mut activity_at, &mut pending);
                break;
            }
        }
    }
}

fn spawn_event_forwarder(handle: tauri::AppHandle, event_rx: mpsc::Receiver<GlobalEvent>) {
    tauri::async_runtime::spawn(run_event_forwarder(handle, event_rx));
}
#[cfg(test)]
mod window_launch_tests {
    use super::{
        coalesce_window_for, is_auth_focus_launch, output_activity_due, OUTPUT_ACTIVITY_INTERVAL,
    };
    use std::time::Instant;

    #[test]
    fn tiny_pty_bursts_use_a_bounded_coalesce_window() {
        assert_eq!(coalesce_window_for(0), 0);
        assert_eq!(coalesce_window_for(255), 0);
        assert_eq!(coalesce_window_for(256), 0);
        assert_eq!(coalesce_window_for(64 * 1024), 0);
    }

    #[test]
    fn output_activity_is_rate_limited_but_resumes_after_quiet_window() {
        let now = Instant::now();
        assert!(output_activity_due(None, now));
        assert!(!output_activity_due(
            Some(now),
            now + OUTPUT_ACTIVITY_INTERVAL / 2
        ));
        assert!(output_activity_due(
            Some(now),
            now + OUTPUT_ACTIVITY_INTERVAL
        ));
    }

    #[test]
    fn only_auth_deep_link_reuses_the_main_window() {
        assert!(is_auth_focus_launch(&[
            "ridge.exe".into(),
            "ridge://auth/focus".into()
        ]));
        assert!(is_auth_focus_launch(&[
            "ridge.exe".into(),
            "RIDGE://AUTH/FOCUS?approved=1".into()
        ]));
        assert!(!is_auth_focus_launch(&["ridge.exe".into()]));
        assert!(!is_auth_focus_launch(&[
            "ridge.exe".into(),
            "C:\\work\\project".into()
        ]));
    }
}
