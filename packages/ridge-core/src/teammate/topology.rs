//! Domain B2 —— Topology 拓扑引擎 + Leader 竞选 + 性格驱动分派。
//!
//! 维护一张「物理空间-逻辑角色-任务依赖」的复合有向图：节点是 [`Teammate`]，
//! 边是正在进行的协同控制流 [`TaskEdge`]。刻意**不引 petgraph**——团队规模小，
//! 手写邻接（`HashMap` 节点 + `Vec` 边）足矣，契合 ridge-core 的克制依赖原则。

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

    // ── Leader ──

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

    /// 动态自组织竞选：按画像权重打分，最高者加冕。
    ///
    /// `W = lang_norm*0.4 + ctx_norm*0.4 + thoroughness*0.2`
    /// （`lang_norm = avg_language_skill/5`，`ctx_norm = min(ctx/200_000, 1)`）。
    /// Observer 不参选。平票时按 id 字典序最小者胜（确定性）。胜者 `Leader`，
    /// 其余非 Observer 降 `Worker`。返回胜者 id；无合格节点返回 `None`。
    pub fn elect_leader(&mut self) -> Option<String> {
        let winner = self
            .nodes
            .values()
            .filter(|t| t.is_eligible())
            .map(|t| (t.id.clone(), leader_weight(t)))
            .fold(None::<(String, f32)>, |best, (id, w)| match best {
                None => Some((id, w)),
                Some((bid, bw)) => {
                    if w > bw || (w == bw && id < bid) {
                        Some((id, w))
                    } else {
                        Some((bid, bw))
                    }
                }
            })
            .map(|(id, _)| id)?;

        for t in self.nodes.values_mut() {
            if t.role == AgentRole::Observer {
                continue;
            }
            t.role = if t.id == winner {
                AgentRole::Leader
            } else {
                AgentRole::Worker
            };
        }
        self.leader = Some(winner.clone());
        Some(winner)
    }

    // ── 性格驱动分派 ──

    /// 挑选擅长某领域、且不是 Leader 的合格 Worker；优先更细致者（因材施教）。
    pub fn pick_worker_by_skill(&self, domain: &str) -> Option<&Teammate> {
        self.nodes
            .values()
            .filter(|t| t.is_eligible() && t.role != AgentRole::Leader)
            .filter(|t| t.capabilities.has_domain_skill(domain))
            .max_by(|a, b| {
                cmp_f32(a.personality.thoroughness, b.personality.thoroughness)
                    .then_with(|| a.id.cmp(&b.id).reverse())
            })
    }

    /// 高风险底层核心库改动 → 挑最稳重（thoroughness 最高、risk 最低）的合格 Worker。
    pub fn pick_cautious_worker(&self) -> Option<&Teammate> {
        self.nodes
            .values()
            .filter(|t| t.is_eligible() && t.role != AgentRole::Leader)
            .max_by(|a, b| {
                cmp_f32(a.personality.thoroughness, b.personality.thoroughness)
                    .then_with(|| {
                        cmp_f32(b.personality.risk_tolerance, a.personality.risk_tolerance)
                    })
                    .then_with(|| a.id.cmp(&b.id).reverse())
            })
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

/// 单个 Teammate 的竞选权重。
fn leader_weight(t: &Teammate) -> f32 {
    let lang_norm = (t.capabilities.avg_language_skill() / 5.0).clamp(0.0, 1.0);
    let ctx_norm = (t.capabilities.context_window as f32 / 200_000.0).min(1.0);
    let thoroughness = t.personality.thoroughness.clamp(0.0, 1.0);
    lang_norm * 0.4 + ctx_norm * 0.4 + thoroughness * 0.2
}

fn cmp_f32(a: f32, b: f32) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teammate::model::{AgentCapabilities, AgentPersonality};

    fn mate(id: &str, ctx: usize, lang: u8, thoroughness: f32, risk: f32) -> Teammate {
        let caps = AgentCapabilities {
            language_skills: [("Rust".to_string(), lang)].into_iter().collect(),
            domain_skills: vec![],
            context_window: ctx,
        };
        Teammate::new(id, id, id.len() as u32)
            .with_capabilities(caps)
            .with_personality(AgentPersonality::new(risk, thoroughness))
    }

    #[test]
    fn election_prefers_big_context_thorough_coder() {
        let mut g = TopologyGraph::new();
        // 大上下文、强编码、稳重 → 应当胜出。
        g.add_teammate(mate("sonnet", 200_000, 5, 0.9, 0.2));
        // 小上下文、快但糙 → 退居 Worker。
        g.add_teammate(mate("haiku", 20_000, 3, 0.3, 0.8));
        let winner = g.elect_leader().unwrap();
        assert_eq!(winner, "sonnet");
        assert_eq!(g.get("sonnet").unwrap().role, AgentRole::Leader);
        assert_eq!(g.get("haiku").unwrap().role, AgentRole::Worker);
        assert_eq!(g.leader_id(), Some("sonnet"));
    }

    #[test]
    fn election_skips_observers() {
        let mut g = TopologyGraph::new();
        let obs = mate("obs", 200_000, 5, 1.0, 0.0).with_role(AgentRole::Observer);
        g.add_teammate(obs);
        g.add_teammate(mate("w", 50_000, 4, 0.6, 0.4));
        let winner = g.elect_leader().unwrap();
        assert_eq!(winner, "w");
        // Observer 角色不被动摇。
        assert_eq!(g.get("obs").unwrap().role, AgentRole::Observer);
    }

    #[test]
    fn election_none_when_only_observers() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("o1", 100, 1, 0.1, 0.1).with_role(AgentRole::Observer));
        assert_eq!(g.elect_leader(), None);
    }

    #[test]
    fn static_leader_demotes_previous() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("a", 100, 1, 0.1, 0.1));
        g.add_teammate(mate("b", 100, 1, 0.1, 0.1));
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
        g.add_teammate(mate("lead", 100, 1, 0.5, 0.5));
        g.add_teammate(mate("wk", 100, 1, 0.5, 0.5));
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
        g.add_teammate(mate("lead", 100, 1, 0.5, 0.5));
        assert_eq!(
            g.delegate("lead", "ghost", TaskEdge::new("i", "x")),
            Err(TopologyError::NodeNotFound("ghost".into()))
        );
    }

    #[test]
    fn remove_drops_edges_and_clears_leader() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("lead", 100, 1, 0.5, 0.5).with_role(AgentRole::Leader));
        g.add_teammate(mate("wk", 100, 1, 0.5, 0.5));
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
        let mut t = mate("x", 100, 1, 0.5, 0.5);
        t.pane_id = 42;
        g.add_teammate(t);
        g.remove_by_pane(42);
        assert!(g.is_empty());
    }

    #[test]
    fn pick_worker_by_skill_prefers_thorough() {
        let mut g = TopologyGraph::new();
        let mut a = mate("a", 100, 4, 0.4, 0.5);
        a.capabilities.domain_skills = vec!["Valgrind".into()];
        let mut b = mate("b", 100, 4, 0.9, 0.5);
        b.capabilities.domain_skills = vec!["Valgrind".into()];
        let mut c = mate("c", 100, 4, 0.99, 0.5);
        c.capabilities.domain_skills = vec!["Git".into()];
        g.add_teammate(a);
        g.add_teammate(b);
        g.add_teammate(c);
        let pick = g.pick_worker_by_skill("valgrind").unwrap();
        assert_eq!(pick.id, "b"); // 有 Valgrind 且更细致
    }

    #[test]
    fn pick_cautious_worker_picks_the_careful_one() {
        let mut g = TopologyGraph::new();
        g.add_teammate(mate("lead", 100, 1, 0.99, 0.0).with_role(AgentRole::Leader));
        g.add_teammate(mate("reckless", 100, 1, 0.2, 0.9));
        g.add_teammate(mate("careful", 100, 1, 0.95, 0.05));
        // Leader 不入选；careful 细致且低风险胜出。
        let pick = g.pick_cautious_worker().unwrap();
        assert_eq!(pick.id, "careful");
    }
}
