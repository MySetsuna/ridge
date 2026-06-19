//! Domain Zero 端侧多智能体协同 —— 桌面 Tauri 命令面.
//!
//! 把 `ridge_core::teammate` 纯核心与运行态侧表桥接成前端可 `invoke` 的命令：
//! - [`get_teammate_topology`] —— D1 Agent Center 侧栏的花名册快照（只读）。
//! - [`resolve_hitl_request`] / [`set_hitl_enabled`] —— D2 HITL 网关裁决与开关。
//! - [`classify_command_risk`] —— 暴露 D2 风险分级器供 UI/调试查询。
//!
//! 拓扑快照从现有 `Workspace` 侧表（`teammate_agent_pane_map` / `_pane_states` /
//! `_pane_titles`）映射，pane 用真实 Uuid 字符串（非 core 内部的 u32）。能力/性格
//! 数据需 register-agent 携带后才能填充 → 当前 role 一律 Worker、leader 为空（后续
//! 接线 §8A）。

use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::state::{AppState, PaneState, Workspace};
use crate::teammate::hitl;

/// 把一个工作区的 teammate 侧表映射为前端 `TopologySnapshot` JSON。
/// `pub(crate)` 以便 teammate HTTP 路由 (`server.rs::route_get_team_profile`) 复用。
pub(crate) fn topology_json(ws: &Workspace) -> Value {
    let roster: Vec<Value> = ws
        .teammate_agent_pane_map
        .iter()
        .map(|(agent_id, pane)| {
            let status = match ws.teammate_pane_states.get(pane) {
                Some(PaneState::Busy) => "Working",
                _ => "Idle",
            };
            let name = ws
                .teammate_pane_titles
                .get(pane)
                .cloned()
                .unwrap_or_else(|| agent_id.clone());
            json!({
                "id": agent_id,
                "name": name,
                "paneId": pane.to_string(),
                "role": "Worker",
                "status": status,
            })
        })
        .collect();
    json!({ "roster": roster, "leaderId": Value::Null, "edges": [] })
}

/// D1 —— 返回某工作区（缺省=活动工作区）的团队拓扑快照。只读。
#[tauri::command]
pub async fn get_teammate_topology(
    workspace_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let wid = match workspace_id {
        Some(s) => Uuid::parse_str(&s).map_err(|e| e.to_string())?,
        None => *state.active_workspace.read(),
    };
    let workspaces = state.workspaces.read();
    let ws = workspaces
        .get(&wid)
        .ok_or_else(|| format!("workspace {wid} not found"))?;
    Ok(topology_json(ws))
}

/// D2 —— 人类对一个挂起的高危动作的裁决回传。
/// `verdict` ∈ {"approve","reject","modify"}；modify 时 `replacement` 为新指令。
#[tauri::command]
pub fn resolve_hitl_request(
    id: String,
    verdict: String,
    replacement: Option<String>,
) -> Result<bool, String> {
    Ok(hitl::resolve(&id, &verdict, replacement))
}

/// D2 —— 开/关 HITL 审批网关（默认关，保持 send-keys 行为零变化）。
#[tauri::command]
pub fn set_hitl_enabled(enabled: bool) -> Result<(), String> {
    hitl::set_enabled(enabled);
    Ok(())
}

/// D2 —— 暴露风险分级器：把一条裸命令行分级为 {level, reason}。
#[tauri::command]
pub fn classify_command_risk(command: String) -> Result<Value, String> {
    serde_json::to_value(ridge_core::classify_shell_command(&command)).map_err(|e| e.to_string())
}
