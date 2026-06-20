//! Domain B2 —— Topology 拓扑（花名册 + 任务委派边 + 静态 Leader）。
//!
//! 维护一张「物理空间-逻辑角色-任务依赖」的复合有向图：节点是 [`Teammate`]，
//! 边是正在进行的协同控制流 [`TaskEdge`]。刻意**不引 petgraph**——团队规模小，
//! 手写邻接（`HashMap` 节点 + `Vec` 边）足矣，契合 ridge-core 的克制依赖原则。
//!
//! 底座化瘦身后**移除了 AI 自动竞选 Leader 与性格驱动分派**（详见
//! specs/2026-06-20-team-agent-upgrade-plan-design.md）：Leader 只由人类静态钦定
//! （[`set_leader_static`](TopologyGraph::set_leader_static)），派活由人/调用方显式发起。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::model::{AgentRole, Teammate, TeammateStatus};

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
#[derive(Debug, Clone, Default)]
pub struct TopologyGraph {
    /// 节点：以 Teammate id 为键。
    nodes: HashMap<String, Teammate>,
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
        self.edges.retain(|(f, t, _)| f != id && t != id);
        if self.leader.as_deref() == Some(id) {
            self.leader = None;
        }
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

    fn mate(id: &str) -> Teammate {
        Teammate::new(id, id, id.len() as u32)
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
}
