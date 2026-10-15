//! OS window lifecycle. Runtime owns Cloud connections and active-session truth.

use tauri::{Emitter, Manager, Runtime, WebviewWindow};
use tauri_plugin_notification::NotificationExt;


/// 前端监听的「登录授权已批准、请立即轮询」事件名（契约 §1）。`ridge://auth/focus`
/// 唤起后由 [`focus_main_window`] 广播；`CloudProModal` 的 `loginViaBrowser` 轮询循环
/// 收到后立即触发一次 poll，免去等待下一个轮询间隔。
pub const AUTH_FOCUS_EVENT: &str = "ridge://auth-focus";

/// Emitted immediately before a native hide. WebView2 may not deliver a DOM
/// `visibilitychange` for an OS-level hide, so the frontend uses this event to
/// release cold terminal renderers and bounded scrollback deterministically.
pub const MEMORY_RECLAIM_EVENT: &str = "ridge://memory-reclaim";

/// Emitted after a tray/deep-root restore so memory-parked terminal renderers
/// can be reattached before the next user interaction.
pub const MEMORY_RESTORE_EVENT: &str = "ridge://memory-restore";

/// Tell the current WebView to release cold renderer/history resources before
/// the native window is hidden. Best effort by design: hiding must still work
/// when a stale WebView has already lost its event channel.
pub fn prepare_for_hide<R: Runtime, T: Emitter<R>>(window: &T) {
    if let Err(error) = window.emit(MEMORY_RECLAIM_EVENT, ()) {
        tracing::debug!(
            target: "ridge::deep_root",
            %error,
            "memory reclaim event unavailable before hide"
        );
    }
}

/// 把主窗口拉回前台（show + unminimize + set_focus）并广播 [`AUTH_FOCUS_EVENT`]。
///
/// 供 deep-link 的 `on_open_url`（`ridge://auth/focus`）与 single-instance 回调复用：
/// 浏览器授权批准后唤起桌面端时调用。窗口若处在「最小化省资源」（hide）态会一并恢复。
/// 每一步失败仅记录，不中断后续步骤——尽力把用户带回前台并触发轮询。
pub fn focus_main_window<R: Runtime>(app: &impl Manager<R>) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!(target: "ridge::deep_root", "auth focus: main window not found");
        return;
    };
    if let Err(e) = window.unminimize() {
        tracing::debug!(target: "ridge::deep_root", error = %e, "auth focus: unminimize");
    }
    if let Err(e) = window.show() {
        tracing::warn!(target: "ridge::deep_root", error = %e, "auth focus: show failed");
    }
    if let Err(e) = window.set_focus() {
        tracing::warn!(target: "ridge::deep_root", error = %e, "auth focus: set_focus failed");
    }
    if let Err(e) = window.emit(AUTH_FOCUS_EVENT, ()) {
        tracing::warn!(target: "ridge::deep_root", error = %e, "auth focus: emit failed");
    }
    tracing::info!(target: "ridge::deep_root", "auth focus: brought main window to front");
}

/// 进入深根模式时发送的原生系统通知文案（据实，**不含**「内存降低 90%」之类的承诺，
/// 因为 v1 是 hide 而非 destroy —— 见模块文档）。
const DEEP_ROOT_NOTIFICATION_TITLE: &str = "Ridge 深根模式";
const DEEP_ROOT_NOTIFICATION_BODY: &str =
    "Ridge 已转入深根模式 🌱，本地渲染窗口已隐藏，您的远程控制通道保持活跃。";

/// 进入深根模式：仅当存在活跃云端远控会话时允许。隐藏（非销毁）主窗口并发原生通知。
///
/// 前端：`invoke('enter_deep_root_mode')`。返回 `Err(String)` 时前端 toast 提示
/// （无活跃远控 / 窗口隐藏失败）。
#[tauri::command]
pub fn enter_deep_root_mode(window: WebviewWindow) -> Result<(), String> {
    // §8.234 触发前置校验：没有活跃远控时进入深根没有意义（窗口隐藏后用户既看不到
    // 本地界面、又没有远端通道接管），直接拒绝让前端 toast。
    let runtime:serde_json::Value=crate::commands::runtime::request("get_cloud_host_state",serde_json::json!({}),true,crate::commands::runtime::window_connection(window.label()))?;
    if !runtime["sessions"].as_array().is_some_and(|sessions|sessions.iter().any(|session|session["state"]=="connected")) {
        return Err("NO_ACTIVE_CLOUD_REMOTE".to_string());
    }

    // Release cold presentation resources before hiding; Kernel owns Cloud sessions.
    prepare_for_hide(&window);
    window
        .hide()
        .map_err(|e| format!("failed to hide window: {e}"))?;

    // 原生系统通知（tauri-plugin-notification 的 Rust 端 API）。通知失败不应让整个
    // 进入深根失败 —— 窗口已经隐藏成功、保活已成立，通知只是锦上添花，记录即可。
    if let Err(e) = window
        .app_handle()
        .notification()
        .builder()
        .title(DEEP_ROOT_NOTIFICATION_TITLE)
        .body(DEEP_ROOT_NOTIFICATION_BODY)
        .show()
    {
        tracing::warn!(
            target: "ridge::deep_root",
            error = %e,
            "deep root notification failed (window already hidden; continuing)"
        );
    }

    tracing::info!(target: "ridge::deep_root", "entered deep root mode (window hidden)");
    Ok(())
}

/// 退出深根模式：显示并聚焦主窗口。托盘「恢复工作台」/双击托盘 与 前端
/// `invoke('restore_from_deep_root')` 共用此命令。
///
/// 前端在窗口重新可见后负责复活渲染循环、接管现有 PTY 增量流（§8.1）。
#[tauri::command]
pub fn restore_from_deep_root(window: WebviewWindow) -> Result<(), String> {
    restore_window(&window)
}

/// 共享实现：show + set_focus。供命令与托盘事件复用，避免逻辑漂移。
/// 泛型化运行时（`R: Runtime`）以便托盘回调（其 `WebviewWindow<R>` 来自泛型
/// `App<R>`/`TrayIcon<R>`）与命令（默认 `Wry`）都能调用同一份实现。
pub fn restore_window<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    window
        .show()
        .map_err(|e| format!("failed to show window: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("failed to focus window: {e}"))?;
    if let Err(error) = window.emit(MEMORY_RESTORE_EVENT, ()) {
        tracing::debug!(
            target: "ridge::deep_root",
            %error,
            "memory restore event unavailable after show"
        );
    }
    tracing::info!(target: "ridge::deep_root", "restored window from deep root mode");
    Ok(())
}
