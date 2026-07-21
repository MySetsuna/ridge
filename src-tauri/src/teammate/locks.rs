//! Domain D3 接线 —— 进程级文件并发写锁 + Tauri 命令。
//!
//! 暴露 [`ridge_core::WriteLockRegistry`] 给运行时：Agent / 编辑流在写文件前
//! `acquire_write_lock`，他人已持→冲突（前端复用 Monaco Diff 弹冲突仲裁视图）。
//! 进程级 `LazyLock`，**不改 `AppState`**。深度接入无 owner 的 fs dispatch 写路径
//! 需先补「写入方身份」（见设计文档 §8C），故当前作为显式命令面提供。

use std::sync::{LazyLock, Mutex};

use ridge_core::{LockOutcome, WriteLockRegistry};

static LOCKS: LazyLock<Mutex<WriteLockRegistry>> =
    LazyLock::new(|| Mutex::new(WriteLockRegistry::new()));

/// 申请写锁：返回 `{ok, outcome, holder?}`。`ok=false` 表示冲突，`holder` 为当前持有者。
#[tauri::command]
pub fn acquire_write_lock(path: String, owner: String) -> Result<serde_json::Value, String> {
    let outcome = LOCKS
        .lock()
        .map_err(|_| "write-lock registry poisoned".to_string())?
        .try_acquire(&path, &owner);
    Ok(match outcome {
        LockOutcome::Acquired => serde_json::json!({ "ok": true, "outcome": "acquired" }),
        LockOutcome::AlreadyHeld => serde_json::json!({ "ok": true, "outcome": "already_held" }),
        LockOutcome::Conflict { holder } => {
            serde_json::json!({ "ok": false, "outcome": "conflict", "holder": holder })
        }
    })
}

/// 释放写锁（仅持有者可释放）。
#[tauri::command]
pub fn release_write_lock(path: String, owner: String) -> Result<bool, String> {
    Ok(LOCKS
        .lock()
        .map_err(|_| "write-lock registry poisoned".to_string())?
        .release(&path, &owner))
}

/// 查询某路径当前持有者。
#[tauri::command]
pub fn write_lock_holder(path: String) -> Result<Option<String>, String> {
    Ok(LOCKS
        .lock()
        .map_err(|_| "write-lock registry poisoned".to_string())?
        .holder(&path)
        .map(|s| s.to_string()))
}
