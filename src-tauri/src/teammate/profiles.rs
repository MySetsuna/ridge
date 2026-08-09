//! Domain B1 接线 —— teammate 名册进程级注册表（底座化瘦身）。
//!
//! `register-agent` 携带的 agent 名 + 被动识别出的能力档落此表（进程级 `LazyLock`，
//! 类比 [`super::hitl`]，**不改 `AppState`**）。`get_teammate_topology` /
//! `route_get_team_profile` 据此构建 `ridge_core::TopologyGraph`。组长不再 AI 自动竞选，
//! 顶层 `leaderId` 恒 null——每个组的组长由用户在前端「编组」里手动指定并持久化。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde_json::{json, Value};
use uuid::Uuid;

use ridge_core::{AgentRole, AgentTier, Teammate, TeammateStatus, TopologyGraph};

struct ProfileEntry {
    teammate: Teammate,
    pane_uuid: Uuid,
}

/// Stable communication-directory projection.  The registry is the only
/// source used by agent write paths to decide whether a target is online.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentContact {
    pub agent_id: String,
    pub name: String,
    pub pane_uuid: Uuid,
    pub status: TeammateStatus,
}

/// `workspace_id → (agent_id → 画像项)`。
static PROFILES: LazyLock<Mutex<HashMap<Uuid, HashMap<String, ProfileEntry>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// register-agent 落花名册条目（名 + 能力档 + Working 态）。
pub fn upsert(
    wid: Uuid,
    agent_id: &str,
    pane_uuid: Uuid,
    name: Option<String>,
    capability: AgentTier,
) -> Result<(), &'static str> {
    if agent_id.trim().is_empty() {
        return Err("agent id is required");
    }
    let mut t = Teammate::new(agent_id, name.unwrap_or_else(|| agent_id.to_string()), 0)
        .with_capability(capability);
    t.status = TeammateStatus::Working;
    let mut g = PROFILES
        .lock()
        .map_err(|_| "agent registry lock poisoned")?;
    g.entry(wid).or_default().insert(
        agent_id.to_string(),
        ProfileEntry {
            teammate: t,
            pane_uuid,
        },
    );
    Ok(())
}

/// 按 pane 移除（release_pane / pane 关闭时，调用方只有 pane_uuid）。
pub fn remove_by_pane(wid: Uuid, pane_uuid: Uuid) -> Vec<String> {
    let Ok(mut g) = PROFILES.lock() else {
        return Vec::new();
    };
    let Some(m) = g.get_mut(&wid) else {
        return Vec::new();
    };
    let removed = m
        .iter()
        .filter(|(_, entry)| entry.pane_uuid == pane_uuid)
        .map(|(agent_id, _)| agent_id.clone())
        .collect::<Vec<_>>();
    m.retain(|_, e| e.pane_uuid != pane_uuid);
    removed
}

/// Remove one confirmed identity without disturbing another Agent sharing a
/// pane during an auto-discovery replacement.  Pane teardown still uses
/// `remove_by_pane` to clear every identity owned by the destroyed pane.
pub fn remove_agent(wid: Uuid, agent_id: &str) -> bool {
    let Ok(mut g) = PROFILES.lock() else {
        return false;
    };
    let Some(entries) = g.get_mut(&wid) else {
        return false;
    };
    let removed = entries.remove(agent_id).is_some();
    if entries.is_empty() {
        g.remove(&wid);
    }
    removed
}

/// Resolve the target immediately before a communication write.  A missing
/// contact means the pane was never confirmed as an Agent or has already been
/// destroyed; callers must fail closed instead of retrying a stale pane.
pub fn target_for_pane(wid: Uuid, pane_uuid: Uuid) -> Option<AgentContact> {
    let g = PROFILES.lock().ok()?;
    let entry = g.get(&wid)?.values().find(|e| e.pane_uuid == pane_uuid)?;
    Some(AgentContact {
        agent_id: entry.teammate.id.clone(),
        name: entry.teammate.name.clone(),
        pane_uuid: entry.pane_uuid,
        status: entry.teammate.status,
    })
}

/// Snapshot the communication directory for diagnostics and preflight checks.
pub fn contacts(wid: Uuid) -> Vec<AgentContact> {
    let Ok(g) = PROFILES.lock() else {
        return Vec::new();
    };
    let mut out = g
        .get(&wid)
        .into_iter()
        .flat_map(|entries| entries.values())
        .map(|entry| AgentContact {
            agent_id: entry.teammate.id.clone(),
            name: entry.teammate.name.clone(),
            pane_uuid: entry.pane_uuid,
            status: entry.teammate.status,
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    out
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

/// 构建该工作区的花名册快照 JSON（`{roster, leaderId, edges}`）。组长不自动竞选：
/// `leaderId` 恒 null、所有成员 role=Worker（每组组长由前端「编组」手动指定）。
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

    // 组长不再由能力自动竞选：交由用户在「编组」里手动为每个组指定组长（前端 localStorage
    // 持久化，见 teammateGroups.svelte.ts）。故顶层花名册无全局 Leader，leaderId 恒 null、
    // 所有成员 role=Worker。
    let leader_id: Option<String> = None;

    let roster: Vec<Value> = graph
        .roster()
        .iter()
        .map(|t| {
            let pane_index = pane_index_by_id
                .get(&t.id)
                .map(|i| json!(i))
                .unwrap_or(Value::Null);
            let pane_id = pane_by_id.get(&t.id).cloned().unwrap_or_default();
            // G1：暂停覆写运行状态（与 topology_json 回退路径同口径）。
            let status = if uuid::Uuid::parse_str(&pane_id)
                .is_ok_and(|p| crate::teammate::suspend::is_suspended(wid, p))
            {
                "Suspended"
            } else {
                status_str(t.status)
            };
            json!({
                "id": t.id,
                "name": t.name,
                "paneId": pane_id,
                "paneIndex": pane_index,
                "role": role_str(t.role),
                "status": status,
                "capability": tier_str(t.capability),
            })
        })
        .collect();

    // 附带该工作区的编组镜像（桌面双写落 workspace-memory），供 remote 只读同步。
    let groups = crate::teammate::memory::dir()
        .map(|d| crate::teammate::memory::get_teammate_groups(d, wid))
        .unwrap_or_else(|| json!([]));
    json!({
        "roster": roster,
        "leaderId": leader_id.map(Value::from).unwrap_or(Value::Null),
        "edges": [],
        "groups": groups,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_registry_resolves_and_removes_targets() {
        let workspace = Uuid::new_v4();
        let pane = Uuid::new_v4();
        upsert(
            workspace,
            "agent-a",
            pane,
            Some("Claude".into()),
            AgentTier::Expert,
        )
        .unwrap();
        assert_eq!(contacts(workspace).len(), 1);
        assert_eq!(
            target_for_pane(workspace, pane).unwrap().agent_id,
            "agent-a"
        );
        assert_eq!(remove_by_pane(workspace, pane), vec!["agent-a"]);
        assert!(target_for_pane(workspace, pane).is_none());
    }

    #[test]
    fn empty_agent_id_never_enters_registry() {
        let result = upsert(Uuid::new_v4(), "  ", Uuid::new_v4(), None, AgentTier::Base);
        assert!(result.is_err());
    }

    #[test]
    fn exact_remove_does_not_drop_other_contacts() {
        let workspace = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let pane_b = Uuid::new_v4();
        upsert(workspace, "agent-a", pane_a, None, AgentTier::Base).unwrap();
        upsert(workspace, "agent-b", pane_b, None, AgentTier::Base).unwrap();
        assert!(remove_agent(workspace, "agent-a"));
        assert!(!contains_agent(workspace, "agent-a"));
        assert!(contains_agent(workspace, "agent-b"));
        remove_by_pane(workspace, pane_b);
    }
}
