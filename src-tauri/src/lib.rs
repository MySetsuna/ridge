mod commands;
mod deep_root;
mod kernel_lifecycle;
pub mod reconnect_policy;
mod state;
mod taskbar;
mod tray;
mod utils;

use std::sync::atomic::{AtomicU64, Ordering};

use crate::commands::{clipboard_files, clipboard_image, project, ridge_file, theme};
use crate::state::AppState;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
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
    let app_data_dir = utils::cdp::local_data_dir();
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

fn start_kernel_bootstrap(app: &tauri::App) {
    let kernel_handle = app.handle().clone();
    let kernel_stop = app.state::<crate::state::AppState>().quitting.clone();
    tauri::async_runtime::spawn(async move {
        let bootstrap = tauri::async_runtime::spawn_blocking(|| {
            let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
            Ok::<_, String>(endpoint)
        })
        .await;
        match bootstrap {
            Ok(Ok(endpoint)) => {
                tracing::info!(target: "ridge::kernel_lifecycle", pid = endpoint.pid, port = endpoint.port, "ridge-kernel ready");
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
    if !utils::cdp::isolated() {
    if let Err(error) = app.deep_link().register_all() {
        tracing::warn!(target: "ridge::deep_link", %error, "deep-link scheme runtime registration failed (continuing)");
    }
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
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(target: "ridge::init", phase = 1, "setup: storing AppHandle");
    let handle = app.handle().clone();
    build_ridge_window(app.handle(), "main", true)?;
    start_kernel_bootstrap(app);
    if let Err(error) = crate::tray::build_tray(app) {
        tracing::error!(target: "ridge::tray", error = %error, "tray init failed");
    }
    crate::taskbar::refresh_jump_list_async(handle.clone());
    register_deep_links(app);
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
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() != "main" {
                return;
        }
        if !state.quitting.load(std::sync::atomic::Ordering::Acquire) {
            if let Err(error) = app.save_window_state(window_state_flags()) {
                tracing::warn!(target: "ridge::init", error = %error, "save window state on hide-to-tray failed");
            }
            ridge_file::save_restore_set();
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
        ridge_file::save_restore_set();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志 + panic hook 尽早装好，后续任何线程 panic 都会落盘到
    // `<LOCALAPPDATA>\ridge\logs\crash-YYYY-MM-DD.log`，便于事故溯源。
    let app_data_dir = utils::cdp::local_data_dir();
    std::fs::create_dir_all(&app_data_dir).ok();
    utils::logging::init_once(&app_data_dir);

    let app_state = AppState::new();

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
        .setup(setup_app)
        .invoke_handler(tauri::generate_handler![
            kernel_lifecycle::bootstrap_runtime_client,
            ridge_file::get_startup_context,
            project::reveal_in_file_manager,
            clipboard_image::read_clipboard_image_to_temp,
            clipboard_files::read_clipboard_file_paths,
            clipboard_files::write_clipboard_file_paths,
            clipboard_files::read_clipboard_sequence,
            deep_root::enter_deep_root_mode,
            deep_root::restore_from_deep_root,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod window_launch_tests {
    use super::is_auth_focus_launch;
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
