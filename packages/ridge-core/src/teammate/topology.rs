//! Domain B2 —— Topology 拓扑（花名册 + 任务委派边 + 静态 Leader）。
//!
//! 维护一张「物理空间-逻辑角色-任务依赖」的复合有向图：节点是 [`Teammate`]，
//! 边是正在进行的协同控制流 [`TaskEdge`]。刻意**不引 petgraph**——团队规模小，
//! 手写邻接（`HashMap` 节点 + `Vec` 边）足矣，契合 ridge-core 的克制依赖原则。
//!
//! 底座化瘦身曾移除整套性格驱动分派与多维加权竞选；此处仅保留两条**极简**定 Leader 路径：
//! 人类静态钦定 [`set_leader_static`](TopologyGraph::set_leader_static)，与依据能力档的
//! 轻量自动竞选 [`elect_leader`]（档最高者当选，同档取 id 最小者，确定性）。派活仍由
//! 人/调用方显式发起。详见 specs/2026-06-20-team-agent-upgrade-plan-design.md。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::communication::{validate_target, AgentIdentity, AgentTarget, CommunicationError};
use super::model::{AgentRole, Teammate, TeammateStatus};

/// 极简 Leader 竞选：能力档（[`AgentTier::rank`](super::model::AgentTier::rank)）最高者当选，
/// 同档以 **id 最小者**打破平局（确定性，避免 HashMap 迭代序抖动）。空名册返回 `None`。
///
/// 返回被选中 Teammate 的 `id`（借用自入参切片）。**注意**：`Teammate::id` 是 `String`
/// （承 register-agent 携带的 `agent_id`，未必是 Uuid），故这里返回 `&str` 而非 `Uuid`，
/// 直接可喂 [`set_leader_static`](TopologyGraph::set_leader_static)。纯函数、零副作用。
pub fn elect_leader(teammates: &[Teammate]) -> Option<&str> {
    teammates
        .iter()
        .max_by(|a, b| {
            a.capability
                .rank()
                .cmp(&b.capability.rank())
                // 同档：id 小者视为「更大」→ max_by 选出它，实现 smallest-id 平局裁决。
                .then_with(|| b.id.cmp(&a.id))
        })
        .map(|t| t.id.as_str())
}

/// 一条任务委派边的载荷。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEdge {
    pub instruction_id: String,
    pub description: String,
}

impl TaskEdge {
    pub fn new(instruction_id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            instruction_id: instruction_id.into(),
            description: description.into(),
        }
    }
}

/// 拓扑操作错误。
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TopologyError {
    #[error("拓扑中不存在节点: {0}")]
    NodeNotFound(String),
}

/// 全局拓扑图：谁在对谁下命令。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopologyGraph {
    /// 节点：以 Teammate id 为键。
    nodes: HashMap<String, Teammate>,
    /// 同一 Kernel roster 中的稳定 Agent identity；旧 roster 文件可缺省。
    #[serde(default)]
    identities: HashMap<String, AgentIdentity>,
    /// Last committed generation per stable Agent id. Tombstones survive
    /// teardown so pane/session reuse cannot accept an old generation.
    #[serde(default)]
    generations: HashMap<String, u64>,
    /// 边：`(from_id, to_id, edge)`。
    edges: Vec<(String, String, TaskEdge)>,
    /// 当前 Leader 的 id。
    leader: Option<String>,
}

impl TopologyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    // ── 节点管理 ──

    /// 插入或替换一个 Teammate（以 id 为键）。
    pub fn add_teammate(&mut self, t: Teammate) {
        if t.role == AgentRole::Leader {
            self.leader = Some(t.id.clone());
        }
        self.nodes.insert(t.id.clone(), t);
    }

    /// 移除节点，并丢弃所有关联边；若它是 Leader 则清空 leader。
    pub fn remove_teammate(&mut self, id: &str) {
        self.nodes.remove(id);
        self.identities.remove(id);
        self.edges.retain(|(f, t, _)| f != id && t != id);
        if self.leader.as_deref() == Some(id) {
            self.leader = None;
        }
    }

    /// Remove only the live identity after a PTY destroy/lease closure has
    /// succeeded; the generation tombstone remains for future fencing.
    pub fn remove_agent_identity_by_pane(&mut self, pane_id: &str) -> Option<AgentIdentity> {
        let agent_id = self
            .identities
            .iter()
            .find(|(_, identity)| identity.pane_id == pane_id)
            .map(|(agent_id, _)| agent_id.clone())?;
        self.identities.remove(&agent_id)
    }

    /// 按物理 Pane ID 移除（Pane 关闭场景）。
    pub fn remove_by_pane(&mut self, pane_id: u32) {
        if let Some(id) = self
            .nodes
            .values()
            .find(|t| t.pane_id == pane_id)
            .map(|t| t.id.clone())
        {
            self.remove_teammate(&id);
        }
    }

    pub fn get(&self, id: &str) -> Option<&Teammate> {
        self.nodes.get(id)
    }

    /// 全体花名册（顺序不保证）。
    pub fn roster(&self) -> Vec<&Teammate> {
        self.nodes.values().collect()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Commit an Agent only after its spawn/attach path has proved it online.
    /// Same generation + lease is idempotent; older generations and leases
    /// cannot replace the active identity.
    pub fn commit_online_agent(
        &mut self,
        identity: AgentIdentity,
    ) -> Result<(), CommunicationError> {
        if identity.agent_id.trim().is_empty()
            || identity.session_id.trim().is_empty()
            || identity.workspace_id.trim().is_empty()
            || identity.pane_id.trim().is_empty()
            || identity.lease.trim().is_empty()
        {
            return Err(CommunicationError::InvalidEnvelope(
                "Agent identity fields must not be empty".into(),
            ));
        }
        if !identity.online || !identity.lifecycle.can_receive() {
            return Err(CommunicationError::TargetOffline(identity.agent_id));
        }
        if let Some(current) = self.identities.get(&identity.agent_id) {
            if identity.generation < current.generation {
                return Err(CommunicationError::GenerationMismatch {
                    expected: current.generation,
                    actual: identity.generation,
                });
            }
            if identity.generation == current.generation && identity.lease != current.lease {
                return Err(CommunicationError::StaleLease);
            }
        } else if let Some(previous) = self.generations.get(&identity.agent_id) {
            if identity.generation <= *previous {
                return Err(CommunicationError::GenerationMismatch {
                    expected: previous.saturating_add(1),
                    actual: identity.generation,
                });
            }
        }
        let agent_id = identity.agent_id.clone();
        self.generations
            .insert(agent_id.clone(), identity.generation);
        self.identities.insert(agent_id, identity);
        Ok(())
    }

    pub fn agent_identity(&self, agent_id: &str) -> Option<&AgentIdentity> {
        self.identities.get(agent_id)
    }

    /// Next generation accepted for a stable Agent id, including after its
    /// live identity has been torn down.
    pub fn next_agent_generation(&self, agent_id: &str) -> u64 {
        self.generations
            .get(agent_id)
            .copied()
            .or_else(|| self.identities.get(agent_id).map(|item| item.generation))
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// Validate against the current bounded Kernel roster snapshot.
    pub fn validate_agent_target(
        &self,
        target: &AgentTarget,
        required_capability: Option<&str>,
    ) -> Result<(), CommunicationError> {
        validate_target(
            target,
            self.identities.get(&target.agent_id),
            required_capability,
        )
    }

    /// Deterministic identity projection for adapters and diagnostics.
    pub fn agent_identities(&self) -> Vec<&AgentIdentity> {
        let mut identities = self.identities.values().collect::<Vec<_>>();
        identities.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        identities
    }

    // ── 边 / 委派 ──

    /// 派活：在图中连一条 `from -> to` 的任务边，并把目标置 `Working`。
    pub fn delegate(
        &mut self,
        from_id: &str,
        to_id: &str,
        edge: TaskEdge,
    ) -> Result<(), TopologyError> {
        if !self.nodes.contains_key(from_id) {
            return Err(TopologyError::NodeNotFound(from_id.to_string()));
        }
        if !self.nodes.contains_key(to_id) {
            return Err(TopologyError::NodeNotFound(to_id.to_string()));
        }
        if let Some(t) = self.nodes.get_mut(to_id) {
            t.status = TeammateStatus::Working;
        }
        self.edges
            .push((from_id.to_string(), to_id.to_string(), edge));
        Ok(())
    }

    /// 某节点发出的所有委派边。
    pub fn edges_from(&self, id: &str) -> Vec<(&str, &TaskEdge)> {
        self.edges
            .iter()
            .filter(|(f, _, _)| f == id)
            .map(|(_, t, e)| (t.as_str(), e))
            .collect()
    }

    /// 边总数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    // ── Leader（仅人类静态钦定）──

    pub fn leader(&self) -> Option<&Teammate> {
        self.leader.as_ref().and_then(|id| self.nodes.get(id))
    }

    pub fn leader_id(&self) -> Option<&str> {
        self.leader.as_deref()
    }

    /// 静态指定 Leader（人类在 `.ridge/workspace.json` 中钦定）。
    /// 前任 Leader 降为 Worker。
    pub fn set_leader_static(&mut self, id: &str) -> Result<(), TopologyError> {
        if !self.nodes.contains_key(id) {
            return Err(TopologyError::NodeNotFound(id.to_string()));
        }
        self.demote_current_leader();
        if let Some(t) = self.nodes.get_mut(id) {
            t.role = AgentRole::Leader;
        }
        self.leader = Some(id.to_string());
        Ok(())
    }

    fn demote_current_leader(&mut self) {
        if let Some(prev) = self.leader.clone() {
            if let Some(t) = self.nodes.get_mut(&prev) {
                if t.role == AgentRole::Leader {
                    t.role = AgentRole::Worker;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teammate::communication::{AgentIdentity, AgentLifecycle};
    use crate::teammate::model::AgentTier;

    fn mate(id: &str) -> Teammate {
        Teammate::new(id, id, id.len() as u32)
    }

    fn tiered(id: &str, tier: AgentTier) -> Teammate {
        mate(id).with_capability(tier)
    }

    fn identity(generation: u64, lease: &str) -> AgentIdentity {
        AgentIdentity {
            agent_id: "agent-a".into(),
            session_id: "session-a".into(),
            workspace_id: "workspace-a".into(),
            pane_id: "pane-a".into(),
            cwd: "C:/work".into(),
            executable: "codex".into(),
            argv: vec!["--resume".into()],
            generation,
            lease: lease.into(),
            lifecycle: AgentLifecycle::Online,
            online: true,
            last_seen_unix_ms: generation,
            capabilities: vec!["messages".into()],
        }
    }

    #[test]
    fn elect_leader_picks_highest_tier() {
        let roster = vec![
            tiered("a", AgentTier::Base),
            tiered("b", AgentTier::Expert),
            tiered("c", AgentTier::Skilled),
        ];
        assert_eq!(elect_leader(&roster), Some("b"));
    }

    #[test]
    fn elect_leader_tiebreaks_on_smallest_id() {
        let roster = vec![
            tiered("bbb", AgentTier::Expert),
            tiered("aaa", AgentTier::Expert),
        ];
        assert_eq!(elect_leader(&roster), Some("aaa"));
    }

    #[test]
    fn elect_leader_single_agent_wins_trivially() {
        let roster = vec![tiered("solo", AgentTier::Base)];
        assert_eq!(elect_leader(&roster), Some("solo"));
    }

    #[test]
    fn elect_leader_none_on_empty() {
        assert_eq!(elect_leader(&[]), None);
    }

    #[test]
    fn elected_leader_reflected_in_roster_roles() {
        let mut g = TopologyGraph::new();
        g.add_teammate(tiered("w", AgentTier::Skilled));
        g.add_teammate(tiered("lead", AgentTier::Expert));
        let roster: Vec<Teammate> = g.roster().into_iter().cloned().collect();
        let winner = elect_leader(&roster).unwrap().to_string();
        g.set_leader_static(&winner).unwrap();
        assert_eq!(g.leader_id(), Some("lead"));
        assert_eq!(g.get("lead").unwrap().role, AgentRole::Leader);
        assert_eq!(g.get("w").unwrap().role, AgentRole::Worker);
    }

    #[test]
    fn static_leader_demotes_previous() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("a"));
        g.add_teammate(mate("b"));
        g.set_leader_static("a").unwrap();
        assert_eq!(g.get("a").unwrap().role, AgentRole::Leader);
        g.set_leader_static("b").unwrap();
        assert_eq!(g.get("a").unwrap().role, AgentRole::Worker);
        assert_eq!(g.get("b").unwrap().role, AgentRole::Leader);
        assert_eq!(g.leader_id(), Some("b"));
    }

    #[test]
    fn set_leader_static_errors_on_missing() {
        let mut g = TopologyGraph::new();
        assert_eq!(
            g.set_leader_static("ghost"),
            Err(TopologyError::NodeNotFound("ghost".into()))
        );
    }

    #[test]
    fn delegate_connects_and_marks_working() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("lead"));
        g.add_teammate(mate("wk"));
        g.delegate("lead", "wk", TaskEdge::new("i1", "分析内存泄漏"))
            .unwrap();
        assert_eq!(g.get("wk").unwrap().status, TeammateStatus::Working);
        let from_lead = g.edges_from("lead");
        assert_eq!(from_lead.len(), 1);
        assert_eq!(from_lead[0].0, "wk");
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn delegate_errors_on_missing_node() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("lead"));
        assert_eq!(
            g.delegate("lead", "ghost", TaskEdge::new("i", "x")),
            Err(TopologyError::NodeNotFound("ghost".into()))
        );
    }

    #[test]
    fn remove_drops_edges_and_clears_leader() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("lead").with_role(AgentRole::Leader));
        g.add_teammate(mate("wk"));
        g.delegate("lead", "wk", TaskEdge::new("i", "x")).unwrap();
        assert_eq!(g.leader_id(), Some("lead"));
        g.remove_teammate("lead");
        assert_eq!(g.leader_id(), None);
        assert_eq!(g.edge_count(), 0);
        assert!(g.get("lead").is_none());
    }

    #[test]
    fn remove_by_pane_works() {
        let mut g = TopologyGraph::new();
        let mut t = mate("x");
        t.pane_id = 42;
        g.add_teammate(t);
        g.remove_by_pane(42);
        assert!(g.is_empty());
    }

    #[test]
    fn topology_round_trips_for_kernel_persistence() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("lead").with_role(AgentRole::Leader));
        let restored: TopologyGraph =
            serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(restored.leader_id(), Some("lead"));
    }

    #[test]
    fn online_identity_commit_is_idempotent_and_teardown_removes_it() {
        let mut graph = TopologyGraph::new();
        graph.commit_online_agent(identity(1, "lease-1")).unwrap();
        graph.commit_online_agent(identity(1, "lease-1")).unwrap();
        assert_eq!(graph.agent_identities().len(), 1);
        assert_eq!(graph.agent_identity("agent-a").unwrap().generation, 1);
        graph.remove_teammate("agent-a");
        assert!(graph.agent_identity("agent-a").is_none());
    }

    #[test]
    fn failed_or_stale_identity_commit_never_replaces_active_entry() {
        let mut graph = TopologyGraph::new();
        graph.commit_online_agent(identity(2, "lease-2")).unwrap();
        let mut failed = identity(3, "lease-3");
        failed.online = false;
        assert!(matches!(
            graph.commit_online_agent(failed),
            Err(CommunicationError::TargetOffline(_))
        ));
        assert_eq!(graph.agent_identity("agent-a").unwrap().generation, 2);
        assert!(matches!(
            graph.commit_online_agent(identity(1, "lease-1")),
            Err(CommunicationError::GenerationMismatch { .. })
        ));
        assert_eq!(graph.agent_identity("agent-a").unwrap().lease, "lease-2");
    }

    #[test]
    fn target_validation_uses_the_same_kernel_roster_snapshot() {
        let mut graph = TopologyGraph::new();
        let current = identity(4, "lease-4");
        let target = current.target();
        graph.commit_online_agent(current).unwrap();
        assert_eq!(
            graph.validate_agent_target(&target, Some("messages")),
            Ok(())
        );
        let mut old = target;
        old.generation = 3;
        assert!(matches!(
            graph.validate_agent_target(&old, None),
            Err(CommunicationError::GenerationMismatch { .. })
        ));
    }

    #[test]
    fn pane_teardown_keeps_generation_tombstone_for_reconnect() {
        let mut graph = TopologyGraph::new();
        graph.commit_online_agent(identity(1, "lease-1")).unwrap();
        assert!(graph.remove_agent_identity_by_pane("pane-a").is_some());
        assert!(graph.agent_identity("agent-a").is_none());
        assert!(matches!(
            graph.commit_online_agent(identity(1, "lease-reused")),
            Err(CommunicationError::GenerationMismatch { .. })
        ));
        graph.commit_online_agent(identity(2, "lease-2")).unwrap();
        assert_eq!(graph.agent_identity("agent-a").unwrap().generation, 2);
    }
}
