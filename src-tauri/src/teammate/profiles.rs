//! Domain B1 接线 —— teammate 名册进程级注册表（底座化瘦身）。
//!
//! `register-agent` 携带的 agent 名落此表（进程级 `LazyLock`，类比 [`super::hitl`]，
//! **不改 `AppState`**）。`get_teammate_topology` / `route_get_team_profile` 据此构建
//! `ridge_core::TopologyGraph` 产出花名册。Leader 不再 AI 竞选（leaderId 恒空，
//! 除非人类静态钦定），能力/性格画像已退场。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde_json::{json, Value};
use uuid::Uuid;

use ridge_core::{AgentRole, Teammate, TeammateStatus, TopologyGraph};

struct ProfileEntry {
    teammate: Teammate,
    pane_uuid: Uuid,
}

/// `workspace_id → (agent_id → 画像项)`。
static PROFILES: LazyLock<Mutex<HashMap<Uuid, HashMap<String, ProfileEntry>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// register-agent 落花名册条目（名 + Working 态）。
pub fn upsert(wid: Uuid, agent_id: &str, pane_uuid: Uuid, name: Option<String>) {
    let mut t = Teammate::new(agent_id, name.unwrap_or_else(|| agent_id.to_string()), 0);
    t.status = TeammateStatus::Working;
    if let Ok(mut g) = PROFILES.lock() {
        g.entry(wid)
            .or_default()
            .insert(agent_id.to_string(), ProfileEntry { teammate: t, pane_uuid });
    }
}

/// 按 pane 移除（release_pane / pane 关闭时，调用方只有 pane_uuid）。
pub fn remove_by_pane(wid: Uuid, pane_uuid: Uuid) {
    if let Ok(mut g) = PROFILES.lock() {
        if let Some(m) = g.get_mut(&wid) {
            m.retain(|_, e| e.pane_uuid != pane_uuid);
        }
    }
}

/// 某工作区是否有画像数据（调用方据此决定用本表还是回退侧表）。
pub fn has(wid: Uuid) -> bool {
    PROFILES
        .lock()
        .map(|g| g.get(&wid).is_some_and(|m| !m.is_empty()))
        .unwrap_or(false)
}

/// 构建该工作区的花名册快照 JSON（`{roster, leaderId, edges}`）。leaderId 恒空（不竞选）。
pub fn topology_for(wid: Uuid) -> Value {
    let empty = json!({ "roster": [], "leaderId": Value::Null, "edges": [] });
    let Ok(g) = PROFILES.lock() else {
        return empty;
    };
    let Some(entries) = g.get(&wid).filter(|m| !m.is_empty()) else {
        return empty;
    };

    let mut graph = TopologyGraph::new();
    let mut pane_by_id: HashMap<String, String> = HashMap::new();
    for (agent_id, e) in entries {
        pane_by_id.insert(agent_id.clone(), e.pane_uuid.to_string());
        graph.add_teammate(e.teammate.clone());
    }
    let leader_id = graph.leader_id().map(|s| s.to_string());

    let roster: Vec<Value> = graph
        .roster()
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "paneId": pane_by_id.get(&t.id).cloned().unwrap_or_default(),
                "role": role_str(t.role),
                "status": status_str(t.status),
            })
        })
        .collect();

    json!({
        "roster": roster,
        "leaderId": leader_id.map(Value::from).unwrap_or(Value::Null),
        "edges": [],
    })
}

fn role_str(r: AgentRole) -> &'static str {
    match r {
        AgentRole::Leader => "Leader",
        AgentRole::Worker => "Worker",
        AgentRole::Observer => "Observer",
    }
}

fn status_str(s: TeammateStatus) -> &'static str {
    match s {
        TeammateStatus::Idle => "Idle",
        TeammateStatus::Working => "Working",
        TeammateStatus::Disappeared => "Disappeared",
    }
}
