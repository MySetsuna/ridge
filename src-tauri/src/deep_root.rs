//! Deep Root Mode（深根模式 🌱）—— 系统托盘 + 主窗口生命周期。
//!
//! 契约权威来源：`docs/contracts/ridge-cloud-protocol.md` §8 / §8.1。
//!
//! # v1 形态（本次交付，必须诚实）
//!
//! 桌面 host 的 WebRTC + E2EE + 信令 WS 目前**活在 WebView/TS**（Agent 2 的
//! `RidgeCloudProvider`）。因此 v1 的「深根」只能用 [`tauri::WebviewWindow::hide`]
//! （**隐藏，不销毁**）：渲染窗口对用户消失，但承载远控连接的 WebView 仍在后台
//! 运行，连接保活成立。内存只是**中等**下降（隐藏的 WebView 不再绘制/合成，
//! 但 JS 堆、WebRTC peer、DataChannel 全部驻留）—— **绝不能宣传「内存降低 90%」**，
//! hide 模式根本达不到，写死会构成虚假宣传（见 §8 末段）。
//!
//! 进入深根的**前置校验**：仅当存在「活跃的云端远控会话」时才允许进入；否则返回
//! `Err`，前端据此 toast 提示「当前没有活跃远控，进入深根模式无意义」。活跃态由前端
//! 通过 [`set_cloud_remote_active`] 上报到 [`AppState::cloud_remote_active`]。
//!
//! # 终态（destroy-based 全量方案）—— 仅文档，**未实现**
//!
//! 真正让内存暴跌的是销毁 WebView：`window.destroy()` 释放整个 WebView 进程
//! （Windows 上是 WebView2 的渲染进程），恢复时再 `WebviewWindowBuilder` 重建。
//! 但这有一个**硬前置条件**：远控连接必须先从 WebView 迁到 Rust 侧
//! （`webrtc-rs`，由 `AppState` 托管），否则销毁 WebView 必然断连，违背「深根仍保活」
//! 的根本诉求（见 §8「目标架构」）。在前置条件未达成前，下面的代码是**注释 stub**，
//! 不要解开 —— 解开会立刻断掉 v1 的 WebView 内远控。
//!
//! ```ignore
//! // ===== destroy-based 全量方案（前置：host WebRTC 已迁到 Rust/AppState）=====
//! //
//! // 进入深根：
//! //   1. 断言 AppState 持有活跃的 Rust 侧 WebRTC 会话句柄（连接已不在 WebView）。
//! //   2. window.destroy()?;            // 释放 WebView2 渲染进程，内存真正暴跌。
//! //      （此后渲染循环不存在；PTY 增量流由 Rust 侧 provider 直接喂给远端。）
//! //   3. 发原生通知：可据实写「本地渲染进程已释放，远程通道由内核保活」。
//! //
//! // 恢复（restore_from_deep_root 的 destroy 分支）：
//! //   1. 用与 lib.rs setup 中一致的 WebviewWindowBuilder 参数重建 "main" 窗口
//! //      （含 initialization_script 注入持久化主题，避免首帧闪烁）。
//! //   2. window.show()? + set_focus()?;
//! //   3. 前端重新挂载后，调用 invoke 把 Rust 侧 provider 已持有的 PTY 增量流
//! //      「接管 / 重放」到新 WebView 的渲染器（复活渲染循环）。
//! //
//! // 关键不变量：连接句柄的所有权在 Rust（AppState），WebView 的生灭不影响它。
//! // 这正是 v1 hide 方案做不到、却是深根模式价值所在的部分。
//! // ============================================================================
//! ```

use std::sync::atomic::Ordering;

use tauri::{Manager, Runtime, State, WebviewWindow};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

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
pub fn enter_deep_root_mode(window: WebviewWindow, state: State<AppState>) -> Result<(), String> {
    // §8.234 触发前置校验：没有活跃远控时进入深根没有意义（窗口隐藏后用户既看不到
    // 本地界面、又没有远端通道接管），直接拒绝让前端 toast。
    if !state.cloud_remote_active.load(Ordering::Acquire) {
        return Err("NO_ACTIVE_CLOUD_REMOTE".to_string());
    }

    // v1：hide（隐藏，不销毁）。连接活在隐藏的 WebView 里，保活成立。
    // destroy-based 全量方案见模块文档（未实现，前置条件未达成）。
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
    tracing::info!(target: "ridge::deep_root", "restored window from deep root mode");
    Ok(())
}

/// 由前端上报「云端远控会话是否活跃」到 [`AppState`]。WebRTC/E2EE provider 活在
/// WebView（v1），Rust 侧无法直接观测连接状态，故由前端在 DataChannel open/close
/// 时调用本命令同步标志，作为 [`enter_deep_root_mode`] 的前置校验依据。
///
/// 前端：`invoke('set_cloud_remote_active', { active })`。
#[tauri::command]
pub fn set_cloud_remote_active(active: bool, state: State<AppState>) -> Result<(), String> {
    state.cloud_remote_active.store(active, Ordering::Release);
    tracing::info!(target: "ridge::deep_root", active, "cloud remote active flag updated");
    Ok(())
}
