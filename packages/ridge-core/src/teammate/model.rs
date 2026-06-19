//! Domain B1 —— Teammate 画像（角色 / 能力矩阵 / 性格）。
//!
//! 每个活跃 Agent 被抽象为一个 [`Teammate`]：物理绑定的 Pane、当前团队角色、
//! 能力矩阵与性格倾向。Topology 引擎（[`super::topology`]）据此竞选 Leader、
//! 因材施教地派活。纯数据模型，零运行时耦合。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 团队角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AgentRole {
    /// 团队领袖（皇冠标记），拥有指挥权。
    Leader,
    /// 执行特定子任务的工人（默认）。
    #[default]
    Worker,
    /// 旁观者，不参与竞选与派活。
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

/// 能力矩阵。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentCapabilities {
    /// 语言技能评分，例如 `{"Rust": 5, "TypeScript": 4}`（1-5 分）。
    pub language_skills: HashMap<String, u8>,
    /// 领域技能，例如 `["Git", "Refactor", "UT-Generation", "Compile-Fix"]`。
    pub domain_skills: Vec<String>,
    /// 上下文窗口大小（token）。
    pub context_window: usize,
}

impl AgentCapabilities {
    /// 语言技能平均分（无技能时为 0）。
    pub fn avg_language_skill(&self) -> f32 {
        if self.language_skills.is_empty() {
            return 0.0;
        }
        let sum: u32 = self.language_skills.values().map(|&v| v as u32).sum();
        sum as f32 / self.language_skills.len() as f32
    }

    /// 是否具备某领域技能（大小写不敏感）。
    pub fn has_domain_skill(&self, skill: &str) -> bool {
        self.domain_skills
            .iter()
            .any(|s| s.eq_ignore_ascii_case(skill))
    }
}

/// 性格倾向。两个维度均钳制在 `[0.0, 1.0]`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct AgentPersonality {
    /// 风险承受度：0.0 极度谨慎 ~ 1.0 激进鲁莽。
    pub risk_tolerance: f32,
    /// 细致度：0.0 粗线条快速交付 ~ 1.0 字斟句酌。
    pub thoroughness: f32,
}

impl AgentPersonality {
    /// 居中性格（0.5 / 0.5）。
    pub fn balanced() -> Self {
        Self {
            risk_tolerance: 0.5,
            thoroughness: 0.5,
        }
    }

    /// 构造并把两维钳制到 `[0.0, 1.0]`。
    pub fn new(risk_tolerance: f32, thoroughness: f32) -> Self {
        Self {
            risk_tolerance: risk_tolerance.clamp(0.0, 1.0),
            thoroughness: thoroughness.clamp(0.0, 1.0),
        }
    }
}

impl Default for AgentPersonality {
    fn default() -> Self {
        Self::balanced()
    }
}

/// 一个活跃 Agent 的完整画像。
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
    pub capabilities: AgentCapabilities,
    pub personality: AgentPersonality,
    pub status: TeammateStatus,
}

impl Teammate {
    /// 以合理默认（Worker / 空能力 / 居中性格 / Idle）构造。
    pub fn new(id: impl Into<String>, name: impl Into<String>, pane_id: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            pane_id,
            role: AgentRole::Worker,
            capabilities: AgentCapabilities::default(),
            personality: AgentPersonality::balanced(),
            status: TeammateStatus::Idle,
        }
    }

    pub fn with_capabilities(mut self, caps: AgentCapabilities) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn with_personality(mut self, personality: AgentPersonality) -> Self {
        self.personality = personality;
        self
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.role = role;
        self
    }

    /// 是否可参与 Leader 竞选 / 被派活（非 Observer）。
    pub fn is_eligible(&self) -> bool {
        self.role != AgentRole::Observer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(skills: &[(&str, u8)], domains: &[&str], ctx: usize) -> AgentCapabilities {
        AgentCapabilities {
            language_skills: skills.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            domain_skills: domains.iter().map(|s| s.to_string()).collect(),
            context_window: ctx,
        }
    }

    #[test]
    fn avg_language_skill_handles_empty() {
        assert_eq!(AgentCapabilities::default().avg_language_skill(), 0.0);
    }

    #[test]
    fn avg_language_skill_averages() {
        let c = caps(&[("Rust", 5), ("TypeScript", 3)], &[], 0);
        assert_eq!(c.avg_language_skill(), 4.0);
    }

    #[test]
    fn has_domain_skill_is_case_insensitive() {
        let c = caps(&[], &["Git", "Refactor"], 0);
        assert!(c.has_domain_skill("git"));
        assert!(c.has_domain_skill("REFACTOR"));
        assert!(!c.has_domain_skill("valgrind"));
    }

    #[test]
    fn personality_clamps() {
        let p = AgentPersonality::new(2.0, -1.0);
        assert_eq!(p.risk_tolerance, 1.0);
        assert_eq!(p.thoroughness, 0.0);
    }

    #[test]
    fn teammate_defaults() {
        let t = Teammate::new("id1", "claude-01", 7);
        assert_eq!(t.role, AgentRole::Worker);
        assert_eq!(t.status, TeammateStatus::Idle);
        assert_eq!(t.personality, AgentPersonality::balanced());
        assert!(t.is_eligible());
    }

    #[test]
    fn builders_chain() {
        let t = Teammate::new("id", "n", 1)
            .with_role(AgentRole::Observer)
            .with_personality(AgentPersonality::new(0.1, 0.95))
            .with_capabilities(caps(&[("Rust", 5)], &["UT-Generation"], 200_000));
        assert_eq!(t.role, AgentRole::Observer);
        assert!(!t.is_eligible());
        assert_eq!(t.capabilities.context_window, 200_000);
        assert!(t.capabilities.has_domain_skill("ut-generation"));
    }

    #[test]
    fn teammate_serde_roundtrip() {
        let t = Teammate::new("id", "n", 2).with_role(AgentRole::Leader);
        let s = serde_json::to_string(&t).unwrap();
        let back: Teammate = serde_json::from_str(&s).unwrap();
        assert_eq!(t, back);
    }
}
