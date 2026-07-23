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
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
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

/// 工作区关闭时清空其全部暂停项（内存侧；sidecar 文件由 [`remove_for`] 删）。
pub fn clear_workspace(wid: Uuid) {
    if let Ok(mut g) = SUSPENDED.lock() {
        g.retain(|(w, _)| *w != wid);
    }
}

// ── M1 切片一（iteration 11）：suspended panes 持久化 ─────────────────────────
// sidecar：`{app_data}/workspace-memory/{wid}.json`，本切片仅
// `{"suspendedPanes":[...],"updatedAt":ms}`（设计 2026-07-23-workspace-memory-design.md
// 选型 B）。**IO 全程 fail-open**：失败只 warn，暂停语义照常、不阻断关闭/启动；
// 损坏 json 载入不 panic（失忆非致命）。原子写 temp+rename。

#[cfg(test)]
fn sidecar_path(dir: &Path, wid: Uuid) -> PathBuf {
    dir.join(format!("{wid}.json"))
}

/// 把某工作区的暂停集写入 `dir`（空集 → 删文件）。dir 注入以便单测。
pub fn persist_to(dir: &Path, wid: Uuid) {
    let panes: Vec<String> = SUSPENDED
        .lock()
        .map(|g| {
            g.iter()
                .filter(|(w, _)| *w == wid)
                .map(|(_, p)| p.to_string())
                .collect()
        })
        .unwrap_or_default();
    // iteration 14 起经 memory.rs doc 级 RMW（不覆写 decisions 等他节；
    // 空集移除本节，doc 空由 memory 删文件）。
    super::memory::update(dir, wid, |doc| {
        if panes.is_empty() {
            doc.remove("suspendedPanes");
        } else {
            doc.insert("suspendedPanes".to_string(), serde_json::json!(panes));
        }
    });
}

/// 启动载入：读 `dir` 下全部 sidecar 重挂注册表。损坏/不可读逐文件跳过。
pub fn load_from(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(wid) = Uuid::parse_str(stem) else { continue };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            tracing::warn!(target: "ridge::suspend", %wid, "sidecar 损坏，跳过（失忆非致命）");
            continue;
        };
        let Some(panes) = value.get("suspendedPanes").and_then(|v| v.as_array()) else { continue };
        for p in panes.iter().filter_map(|v| v.as_str()) {
            if let Ok(pane) = Uuid::parse_str(p) {
                suspend(wid, pane);
            }
        }
    }
}

/// 工作区关闭清理：删内存项 + sidecar 文件（best-effort，整 doc 随区亡）。
pub fn remove_from(dir: &Path, wid: Uuid) {
    clear_workspace(wid);
    super::memory::remove(dir, wid);
}

// iteration 14：dir 解析统一走 memory::dir()（lib.rs setup 注入一次），
// 删除旧 state.app_handle 双解析路径。未注入（纯单测）→ no-op（fail-open）。

/// 变更后落盘（suspend/resume/clear 的调用方钩这里）。
pub fn persist_for(wid: Uuid) {
    if let Some(dir) = super::memory::dir() {
        persist_to(dir, wid);
    }
}

/// 应用启动恢复入口（lib.rs setup 调一次）。
pub fn load_all_for() {
    if let Some(dir) = super::memory::dir() {
        load_from(dir);
    }
}

/// 工作区关闭清理入口（`close_workspace_core` 单点调）。
pub fn remove_for(wid: Uuid) {
    match super::memory::dir() {
        Some(dir) => remove_from(dir, wid),
        None => clear_workspace(wid),
    }
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

    /// M1 切片一：持久化↔载入闭环 + 空集删文件 + 损坏容忍 + 关区清理（dir 注入，
    /// 不依赖真实 app 目录；全程 fail-open 语义）。
    #[test]
    fn sidecar_roundtrip_corruption_and_cleanup() {
        let dir = std::env::temp_dir().join(format!("ridge-suspend-test-{}", Uuid::new_v4()));
        let (w1, p1) = (Uuid::new_v4(), Uuid::new_v4());

        // 持久化 → 模拟重启（内存清）→ 载入恢复。
        suspend(w1, p1);
        persist_to(&dir, w1);
        assert!(sidecar_path(&dir, w1).exists());
        resume(w1, p1);
        assert!(!is_suspended(w1, p1));
        load_from(&dir);
        assert!(is_suspended(w1, p1), "重启载入后暂停态必须恢复");

        // resume 后落盘 → 空集删文件 → 再载不复活。
        resume(w1, p1);
        persist_to(&dir, w1);
        assert!(!sidecar_path(&dir, w1).exists());
        load_from(&dir);
        assert!(!is_suspended(w1, p1));

        // 损坏 json：载入不 panic、不产生条目。
        let w2 = Uuid::new_v4();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(sidecar_path(&dir, w2), "not-json{{{").unwrap();
        load_from(&dir);

        // 关区清理：内存 + 文件同除。
        let (w3, p3) = (Uuid::new_v4(), Uuid::new_v4());
        suspend(w3, p3);
        persist_to(&dir, w3);
        remove_from(&dir, w3);
        assert!(!is_suspended(w3, p3));
        assert!(!sidecar_path(&dir, w3).exists());

        let _ = std::fs::remove_dir_all(&dir);
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
