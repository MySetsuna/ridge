//! Domain D3 接线 —— 进程级 per-pane 循环熔断器，触发即 SIGINT + 通知。
//!
//! Worker 经 `report-progress`（带 `pane` + 失败特征 `key`）喂结果；当某 pane 连续
//! 相似失败达阈值（[`ridge_core::LoopBreaker`]，默认 3）→ 判定逻辑死锁：向该 pane
//! PTY 写 `Ctrl+C`(0x03) 强制中断，并 emit `teammate://circuit-tripped` 让前端/Leader
//! 接管。进程级 `LazyLock`，**不改 `AppState`**。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tauri::Emitter;
use uuid::Uuid;

use ridge_core::{LoopBreaker, LoopSignal};

use crate::state::AppState;

/// 熔断事件名（前端可监听以提示「Worker 死锁已熔断」）。
pub const CIRCUIT_EVENT: &str = "teammate://circuit-tripped";

static BREAKERS: LazyLock<Mutex<HashMap<(Uuid, Uuid), LoopBreaker>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 记录一次 worker 动作结果。`failed=false` 清零；连续相似失败达阈值则熔断：
/// 向 pane 发 Ctrl+C + emit 事件 + 复位该 breaker。
pub fn record(
    handle: &tauri::AppHandle,
    state: &AppState,
    wid: Uuid,
    pane: Uuid,
    key: &str,
    failed: bool,
) {
    let tripped = {
        let Ok(mut g) = BREAKERS.lock() else {
            return;
        };
        let b = g
            .entry((wid, pane))
            .or_insert_with(LoopBreaker::with_default);
        let sig = if failed {
            LoopSignal::failure(key)
        } else {
            LoopSignal::success()
        };
        b.record(&sig)
    };

    if tripped {
        // 硬件级中断：向该 pane PTY 写 Ctrl+C。
        let _ = crate::commands::terminal::write_pty_bytes_workspace(state, wid, pane, &[0x03]);
        let _ = handle.emit(
            CIRCUIT_EVENT,
            serde_json::json!({
                "workspaceId": wid.to_string(),
                "paneId": pane.to_string(),
                "reason": key,
            }),
        );
        if let Ok(mut g) = BREAKERS.lock() {
            if let Some(b) = g.get_mut(&(wid, pane)) {
                b.reset();
            }
        }
    }
}

/// pane 关闭 / 释放时清理其 breaker。
pub fn forget_pane(wid: Uuid, pane: Uuid) {
    if let Ok(mut g) = BREAKERS.lock() {
        g.remove(&(wid, pane));
    }
}
