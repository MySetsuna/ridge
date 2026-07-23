//! G1 阶段一 —— Agent 软暂停/恢复（设计：`docs/superpowers/specs/2026-07-23-agent-suspend-resume-design.md`）.
//!
//! **输入门控（L-b）**：suspended pane 的 **agent 写路径**（send-keys / delegate 提示注入 /
//! MCP 文本注入 / exec 命令注入）统一经 [`agent_pty_write`] 收口拒绝；**桌面人类输入
//! （`write_to_pty`）不受限**——接管语义。断路器 Ctrl-C（circuit.rs）为安全刹车，
//! 刻意**不**门控。OS 级真冻结属阶段二，本模块不做。
//!
//! 注册表进程全局（类比 [`super::hitl`]，不改 `AppState`），键 =（workspace, pane）。
//! pane 释放 / agent 注销时由调用方清理（`release_teammate_agent` 路径）。

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use uuid::Uuid;

static SUSPENDED: LazyLock<Mutex<HashSet<(Uuid, Uuid)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// 暂停某 pane 的 agent 输入。幂等：重复暂停返回 Ok。
pub fn suspend(wid: Uuid, pane: Uuid) {
    if let Ok(mut g) = SUSPENDED.lock() {
        g.insert((wid, pane));
    }
}

/// 恢复。幂等：未暂停时 no-op。
pub fn resume(wid: Uuid, pane: Uuid) {
    if let Ok(mut g) = SUSPENDED.lock() {
        g.remove(&(wid, pane));
    }
}

pub fn is_suspended(wid: Uuid, pane: Uuid) -> bool {
    SUSPENDED.lock().map(|g| g.contains(&(wid, pane))).unwrap_or(false)
}

/// pane 关闭 / agent 注销时清理（与 `resume` 同效，语义命名区分调用意图）。
pub fn clear_pane(wid: Uuid, pane: Uuid) {
    resume(wid, pane);
}

/// **Agent 写路径唯一收口**：suspended → 拒绝（明确错误，agent 可读）；否则透传
/// `write_pty_bytes_workspace`。teammate server 的全部 agent 注入点必须走这里，
/// 不得直呼 terminal 写函数（G1 阶段一合同不变量）。
pub fn agent_pty_write(
    state: &crate::state::AppState,
    wid: Uuid,
    pane: Uuid,
    bytes: &[u8],
) -> Result<(), String> {
    if is_suspended(wid, pane) {
        return Err("agent suspended: input gated by user (resume to continue)".to_string());
    }
    crate::commands::terminal::write_pty_bytes_workspace(state, wid, pane, bytes)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G1 门控：suspended → 明确拒绝错误（不触 PTY）；resume 后放行至 terminal 层
    /// （测试态无 PTY，落到「不同的」pane 查找错误——足以区分门控与透传）。
    #[test]
    fn agent_pty_write_gates_suspended_pane() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let state = crate::state::AppState::new(tx);
        let wid = state.active_workspace_id();
        let pane = Uuid::new_v4();

        suspend(wid, pane);
        let gated = agent_pty_write(&state, wid, pane, b"echo hi\n").unwrap_err();
        assert!(gated.starts_with("agent suspended"), "unexpected: {gated}");

        resume(wid, pane);
        let passed = agent_pty_write(&state, wid, pane, b"echo hi\n").unwrap_err();
        assert!(!passed.starts_with("agent suspended"), "still gated: {passed}");
    }

    #[test]
    fn suspend_resume_idempotent_and_scoped() {
        let (w1, p1) = (Uuid::new_v4(), Uuid::new_v4());
        let (w2, p2) = (Uuid::new_v4(), Uuid::new_v4());
        assert!(!is_suspended(w1, p1));
        suspend(w1, p1);
        suspend(w1, p1); // 幂等
        assert!(is_suspended(w1, p1));
        assert!(!is_suspended(w2, p2)); // 键隔离
        assert!(!is_suspended(w1, p2));
        resume(w1, p1);
        resume(w1, p1); // 幂等
        assert!(!is_suspended(w1, p1));
        suspend(w2, p2);
        clear_pane(w2, p2);
        assert!(!is_suspended(w2, p2));
    }
}
