//! Domain B1 —— Teammate 名册数据（角色 / 运行态）。
//!
//! 每个活跃 Agent 被抽象为一个 [`Teammate`]：物理绑定的 Pane、当前团队角色、
//! 运行态。底座化瘦身后**不再承载能力矩阵/性格画像**（那些只服务于已退场的
//! Leader 竞选与性格分派，见 specs/2026-06-20-team-agent-upgrade-plan-design.md）：
//! 这里只保留「给人看的」花名册所需的最小字段。纯数据模型，零运行时耦合。

use serde::{Deserialize, Serialize};

/// 团队角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AgentRole {
    /// 团队领袖（皇冠标记）。底座化后仅由人类静态钦定，不再 AI 自动竞选。
    Leader,
    /// 执行特定子任务的工人（默认）。
    #[default]
    Worker,
    /// 旁观者，不参与派活。
    Observer,
}

/// 运行态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TeammateStatus {
    /// 空闲（默认）。
    #[default]
    Idle,
    /// 正在执行任务。
    Working,
    /// Pane 关闭 / 失联。
    Disappeared,
}

/// 一个活跃 Agent 的花名册条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Teammate {
    /// 唯一标识（UUID 或 Pane_ID 衍生）。
    pub id: String,
    /// 智能体代号（如 "claude-code-01"）。
    pub name: String,
    /// 物理绑定的 Ridge Pane ID。
    pub pane_id: u32,
    /// 当前团队角色。
    pub role: AgentRole,
    pub status: TeammateStatus,
}

impl Teammate {
    /// 以合理默认（Worker / Idle）构造。
    pub fn new(id: impl Into<String>, name: impl Into<String>, pane_id: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            pane_id,
            role: AgentRole::Worker,
            status: TeammateStatus::Idle,
        }
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.role = role;
        self
    }

    /// 是否可被派活（非 Observer）。
    pub fn is_eligible(&self) -> bool {
        self.role != AgentRole::Observer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teammate_defaults() {
        let t = Teammate::new("id1", "claude-01", 7);
        assert_eq!(t.role, AgentRole::Worker);
        assert_eq!(t.status, TeammateStatus::Idle);
        assert!(t.is_eligible());
    }

    #[test]
    fn role_builder_and_eligibility() {
        let t = Teammate::new("id", "n", 1).with_role(AgentRole::Observer);
        assert_eq!(t.role, AgentRole::Observer);
        assert!(!t.is_eligible());
    }

    #[test]
    fn teammate_serde_roundtrip() {
        let t = Teammate::new("id", "n", 2).with_role(AgentRole::Leader);
        let s = serde_json::to_string(&t).unwrap();
        let back: Teammate = serde_json::from_str(&s).unwrap();
        assert_eq!(t, back);
    }
}
