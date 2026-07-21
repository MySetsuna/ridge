//! Domain B1 接线 —— teammate 名册进程级注册表（底座化瘦身）。
//!
//! `register-agent` 携带的 agent 名 + 被动识别出的能力档落此表（进程级 `LazyLock`，
//! 类比 [`super::hitl`]，**不改 `AppState`**）。`get_teammate_topology` /
//! `route_get_team_profile` 据此构建 `ridge_core::TopologyGraph`，并跑极简能力竞选
//! （[`ridge_core::elect_leader`]）产出带真实 `leaderId` + Leader 角色的花名册。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde_json::{json, Value};
use uuid::Uuid;

use ridge_core::{elect_leader, AgentRole, AgentTier, Teammate, TeammateStatus, TopologyGraph};

struct ProfileEntry {
    teammate: Teammate,
    pane_uuid: Uuid,
}

/// `workspace_id → (agent_id → 画像项)`。
static PROFILES: LazyLock<Mutex<HashMap<Uuid, HashMap<String, ProfileEntry>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// register-agent 落花名册条目（名 + 能力档 + Working 态）。
pub fn upsert(wid: Uuid, agent_id: &str, pane_uuid: Uuid, name: Option<String>, capability: AgentTier) {
    let mut t = Teammate::new(agent_id, name.unwrap_or_else(|| agent_id.to_string()), 0)
        .with_capability(capability);
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

/// 该工作区花名册是否含某 `agent_id`（`ridge_join_group` 加成员前校验目标存在）。
pub fn contains_agent(wid: Uuid, agent_id: &str) -> bool {
    PROFILES
        .lock()
        .map(|g| g.get(&wid).is_some_and(|m| m.contains_key(agent_id)))
        .unwrap_or(false)
}

/// 由 pane_uuid 反查其 `agent_id`（`ridge_join_group` 支持按 pane 寻址成员时用）。
pub fn agent_id_for_pane(wid: Uuid, pane_uuid: Uuid) -> Option<String> {
    let g = PROFILES.lock().ok()?;
    let m = g.get(&wid)?;
    m.iter()
        .find(|(_, e)| e.pane_uuid == pane_uuid)
        .map(|(agent_id, _)| agent_id.clone())
}

/// 构建该工作区的花名册快照 JSON（`{roster, leaderId, edges}`）。跑极简能力竞选
/// （[`elect_leader`]）产出真实 `leaderId`，并把当选者角色标 Leader、余者 Worker。
///
/// `pane_order` 为该工作区当前叶子序列（`pane_tree.get_all_leaves()`，调用方持
/// `AppState` 后读出传入）。据此为每个成员补出数字 `paneIndex`，与 MCP 数字索引
/// 寻址同源，使花名册的 `paneId`(Uuid) 与 `paneIndex`(数字) 两键都可寻址（缺口1）。
pub fn topology_for(wid: Uuid, pane_order: &[Uuid]) -> Value {
    let empty = json!({ "roster": [], "leaderId": Value::Null, "edges": [] });
    let Ok(g) = PROFILES.lock() else {
        return empty;
    };
    let Some(entries) = g.get(&wid).filter(|m| !m.is_empty()) else {
        return empty;
    };

    let mut graph = TopologyGraph::new();
    let mut pane_by_id: HashMap<String, String> = HashMap::new();
    let mut pane_index_by_id: HashMap<String, usize> = HashMap::new();
    for (agent_id, e) in entries {
        pane_by_id.insert(agent_id.clone(), e.pane_uuid.to_string());
        if let Some(idx) = pane_order.iter().position(|p| *p == e.pane_uuid) {
            pane_index_by_id.insert(agent_id.clone(), idx);
        }
        graph.add_teammate(e.teammate.clone());
    }

    // 极简能力竞选：档最高者当选组长（同档取 id 最小者）。set_leader_static 顺带把
    // 当选者角色置 Leader、前任降 Worker，故下方 roster 的 role 直接反映竞选结果。
    let roster_for_vote: Vec<Teammate> = graph.roster().into_iter().cloned().collect();
    if let Some(winner) = elect_leader(&roster_for_vote).map(|s| s.to_string()) {
        let _ = graph.set_leader_static(&winner);
    }
    let leader_id = graph.leader_id().map(|s| s.to_string());

    let roster: Vec<Value> = graph
        .roster()
        .iter()
        .map(|t| {
            let pane_index = pane_index_by_id
                .get(&t.id)
                .map(|i| json!(i))
                .unwrap_or(Value::Null);
            json!({
                "id": t.id,
                "name": t.name,
                "paneId": pane_by_id.get(&t.id).cloned().unwrap_or_default(),
                "paneIndex": pane_index,
                "role": role_str(t.role),
                "status": status_str(t.status),
                "capability": tier_str(t.capability),
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

fn tier_str(t: AgentTier) -> &'static str {
    match t {
        AgentTier::Base => "Base",
        AgentTier::Skilled => "Skilled",
        AgentTier::Expert => "Expert",
    }
}
