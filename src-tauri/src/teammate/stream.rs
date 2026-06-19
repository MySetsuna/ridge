//! Domain A3 接线 —— PTY 读路径 StreamCleaner 网关。
//!
//! 把 [`ridge_core::StreamCleaner`] 接到桌面终端读热路径（`engine::pty::spawn_pty_reader`）：
//! 隐藏 Agent 互灌的 TML 控制区间（MUTATION_HIDE），并把完整解析出的 TML 消息经
//! `teammate://tml-message` 上抛（喂 D1 Agent Center 审计）。
//!
//! **默认关闭**（`ENABLED=false`）：[`apply`] 在关闭时**原样返回**，热路径字节级零变化
//! ——这是终端输出的关键路径，误净化会污染所有显示，故 flag 默认关，待 `tauri:dev:cdp`
//! 真机回归（本机 WebView2 CDP 故障期间无法自验）通过后再 `set_tml_stream_enabled(true)`。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Emitter;

use ridge_core::StreamCleaner;

use crate::state::AppState;

/// 解析出的 TML 消息事件名（前端 `teammateModel.parseTmlMessage` 消费）。
pub const TML_MESSAGE_EVENT: &str = "teammate://tml-message";

static ENABLED: AtomicBool = AtomicBool::new(false);

/// 网关是否开启（默认关）。
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 在 PTY 读路径净化一段**已解码**文本。
///
/// - 关闭 → 原样返回 `raw`（仅一次 atomic load，热路径零开销/零行为变化）。
/// - 开启 → 经 `cleaner` 隐藏 TML 区间、对每条解析出的消息 emit `teammate://tml-message`，
///   返回可见文本（可能为空，调用方据此跳过）。
///
/// `cleaner` 由调用方按 pane 持有并跨读循环复用（TML 标记可能跨 chunk 切分）。
pub fn apply(state: &AppState, cleaner: &mut StreamCleaner, raw: String) -> String {
    if !is_enabled() {
        return raw;
    }
    let out = cleaner.clean_stream(raw.as_bytes());
    if !out.messages.is_empty() {
        if let Some(h) = state.app_handle.get() {
            for msg in &out.messages {
                let _ = h.emit(
                    TML_MESSAGE_EVENT,
                    serde_json::to_value(msg).unwrap_or_default(),
                );
            }
        }
    }
    String::from_utf8_lossy(&out.visible).into_owned()
}

/// 开/关 TML 流净化网关（默认关，保持终端读路径字节级零变化）。
#[tauri::command]
pub fn set_tml_stream_enabled(enabled: bool) -> Result<(), String> {
    ENABLED.store(enabled, Ordering::Relaxed);
    Ok(())
}
