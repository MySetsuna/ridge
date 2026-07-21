//! 外部主机注册表（「主机 / Hosts」面板的远端 ridge / rdg host）。
//!
//! P3/P4 基础层：本模块承载**已登记的远端主机**及其会话元数据、连接状态，并暴露
//! `host_list_snapshot` / `connect_host` / `disconnect_host` / `forget_host` 命令面。
//!
//! **边界（本里程）**：这里只做主机登记与状态管理；真正的**出站连接 + 远端 PTY 流
//! 接管**（把远端 pane 当本地 foreign pane，经 `PtyHandle.remote_ref` 路由 I/O）是
//! 明确的下一里程，需 rebuild + 一台真实远端主机联调验证。见
//! `docs/superpowers/specs/2026-06-30-multi-host-foreign-terminal-hosts-design.md` §2/§9。

use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;

use crate::state::AppState;
use tauri::State;

/// 主机类型：远端 ridge（LAN/cloud）或 rdg（ridge-cli headless host）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Remote,
    Rdg,
}

/// 主机连接状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

/// 远端主机上的一个会话（pane）的元数据。
#[derive(Clone, Debug, Serialize)]
pub struct HostSessionMeta {
    /// 远端 pane id（provider 域内）。
    pub id: String,
    pub title: String,
    /// 是否已被本地某工作区领养。
    pub attached: bool,
}

/// 一台已登记的远端主机记录（序列化给前端 Hosts 面板）。**不含凭据**。
#[derive(Clone, Debug, Serialize)]
pub struct HostRecord {
    pub id: String,
    pub kind: HostKind,
    pub label: String,
    /// 地址（`ip:port` 或 rdg 地址）。凭据（token/TOTP）故意不落库、不序列化。
    pub addr: String,
    pub status: HostStatus,
    /// 面向用户的状态说明（面板顶部/主机行提示）。
    pub detail: String,
    pub sessions: Vec<HostSessionMeta>,
}

/// 一个 foreign pane 指向的远端会话引用（`PtyHandle.remote_ref`）。live 传输里程接线。
#[derive(Clone, Debug)]
pub struct RemoteRef {
    pub host_id: String,
    pub host_label: String,
    pub remote_pane_id: String,
    pub kind: HostKind,
}

/// 进程内主机注册表（AppState 持有 `Arc<HostRegistry>`）。
#[derive(Default)]
pub struct HostRegistry {
    hosts: RwLock<HashMap<String, HostRecord>>,
}

impl HostRegistry {
    pub fn snapshot(&self) -> Vec<HostRecord> {
        let mut v: Vec<HostRecord> = self.hosts.read().values().cloned().collect();
        v.sort_by(|a, b| a.label.cmp(&b.label));
        v
    }

    pub fn upsert(&self, rec: HostRecord) {
        self.hosts.write().insert(rec.id.clone(), rec);
    }

    pub fn remove(&self, id: &str) -> bool {
        self.hosts.write().remove(id).is_some()
    }

    pub fn set_status(&self, id: &str, status: HostStatus, detail: impl Into<String>) {
        if let Some(h) = self.hosts.write().get_mut(id) {
            h.status = status;
            h.detail = detail.into();
        }
    }
}

const LIVE_TRANSPORT_PENDING: &str =
    "已登记主机配置。远端 PTY 流接管（live 传输）为下一里程，需 rebuild + 真实主机联调。";

/// 快照所有已登记远端主机（读，供前端 Hosts 面板与 headless 会话合并展示）。
#[tauri::command]
pub fn host_list_snapshot(state: State<'_, AppState>) -> Vec<HostRecord> {
    state.hosts.snapshot()
}

/// 登记一台远端主机。凭据（`token`）仅预留给 live 传输，**此处不落库、不回传**。
/// 当前只登记 + 置 `Disconnected` 状态；真正出站连接在 live 传输里程接入。
#[tauri::command]
pub fn connect_host(
    state: State<'_, AppState>,
    kind: String,
    label: Option<String>,
    addr: String,
    token: Option<String>,
) -> Result<String, String> {
    let addr = addr.trim().to_string();
    if addr.is_empty() {
        return Err("地址不能为空".to_string());
    }
    // 凭据不落库（避免序列化泄漏）。live 传输里程会在连接任务里就地使用。
    let _ = token;
    let kind = match kind.as_str() {
        "rdg" => HostKind::Rdg,
        _ => HostKind::Remote,
    };
    let id = format!("{}:{}", if kind == HostKind::Rdg { "rdg" } else { "lan" }, addr);
    let label = label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.clone());
    state.hosts.upsert(HostRecord {
        id: id.clone(),
        kind,
        label,
        addr,
        status: HostStatus::Disconnected,
        detail: LIVE_TRANSPORT_PENDING.to_string(),
        sessions: Vec::new(),
    });
    Ok(id)
}

/// 断开一台远端主机（置 `Disconnected`；不移除登记）。
#[tauri::command]
pub fn disconnect_host(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    state
        .hosts
        .set_status(&host_id, HostStatus::Disconnected, "已断开");
    Ok(())
}

/// 忘记一台远端主机（移除登记）。
#[tauri::command]
pub fn forget_host(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    state.hosts.remove(&host_id);
    Ok(())
}
