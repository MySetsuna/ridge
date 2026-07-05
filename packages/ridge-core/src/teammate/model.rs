//! Domain B1 —— Teammate 名册数据（角色 / 运行态）。
//!
//! 每个活跃 Agent 被抽象为一个 [`Teammate`]：物理绑定的 Pane、当前团队角色、
//! 运行态、以及一个**极简能力档** [`AgentTier`]。底座化瘦身曾移除整套能力矩阵/
//! 性格画像（见 specs/2026-06-20-team-agent-upgrade-plan-design.md）；此处仅重新引入
//! 一个「够用即止」的三档能力位，供 [`super::topology::elect_leader`] 轻量竞选出组长，
//! **不复活**语言分维/性格向量那套重物。纯数据模型，零运行时耦合。

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

/// 极简能力档（**刻意只三档**，取代已退场的多维能力矩阵）。
///
/// 竞选组长时用 [`rank`](AgentTier::rank) 排序：档高者优先。仅凭 agent 名 / 启动程序
/// 关键字被动识别（[`recognize_capability`]），不做任何能力探测或握手协商。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AgentTier {
    /// 未知 / 轻量模型（默认）。
    #[default]
    Base,
    /// 有一定综合能力的模型（如 GPT / Codex / Sonnet / Gemini 系）。
    Skilled,
    /// 顶配模型（如 Claude / Opus 系），最宜担纲组长。
    Expert,
}

impl AgentTier {
    /// 竞选权重（越大越强）：Base=0 / Skilled=1 / Expert=2。
    pub fn rank(&self) -> u8 {
        match self {
            AgentTier::Base => 0,
            AgentTier::Skilled => 1,
            AgentTier::Expert => 2,
        }
    }
}

/// 被动识别：仅凭 agent 展示名 + 可选启动程序名的**大小写不敏感**关键字匹配定档。
///
/// - 含 `claude` / `opus` → [`AgentTier::Expert`]（顶配优先，即便同时含 `sonnet`）；
/// - 含 `codex` / `gpt` / `sonnet` / `gemini` → [`AgentTier::Skilled`]；
/// - 其余 → [`AgentTier::Base`]。
///
/// 纯函数、零副作用；识别不到就落 Base（单 agent 场景仍能平凡当选组长）。
pub fn recognize_capability(name: &str, program: Option<&str>) -> AgentTier {
    let hay = format!("{} {}", name, program.unwrap_or("")).to_lowercase();
    let has = |kw: &str| hay.contains(kw);
    if has("claude") || has("opus") {
        AgentTier::Expert
    } else if has("codex") || has("gpt") || has("sonnet") || has("gemini") {
        AgentTier::Skilled
    } else {
        AgentTier::Base
    }
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
    /// 被动识别出的能力档（默认 [`AgentTier::Base`]），供轻量组长竞选排序。
    #[serde(default)]
    pub capability: AgentTier,
}

impl Teammate {
    /// 以合理默认（Worker / Idle / Base）构造。
    pub fn new(id: impl Into<String>, name: impl Into<String>, pane_id: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            pane_id,
            role: AgentRole::Worker,
            status: TeammateStatus::Idle,
            capability: AgentTier::Base,
        }
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.role = role;
        self
    }

    /// 链式设定能力档。
    pub fn with_capability(mut self, capability: AgentTier) -> Self {
        self.capability = capability;
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
        let t = Teammate::new("id", "n", 2)
            .with_role(AgentRole::Leader)
            .with_capability(AgentTier::Expert);
        let s = serde_json::to_string(&t).unwrap();
        let back: Teammate = serde_json::from_str(&s).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn teammate_defaults_to_base_tier() {
        assert_eq!(Teammate::new("id", "n", 0).capability, AgentTier::Base);
    }

    #[test]
    fn tier_rank_is_ordered() {
        assert!(AgentTier::Expert.rank() > AgentTier::Skilled.rank());
        assert!(AgentTier::Skilled.rank() > AgentTier::Base.rank());
    }

    #[test]
    fn recognize_capability_matches_keywords_case_insensitively() {
        // Expert: claude / opus (顶配优先，即便同时含 sonnet)。
        assert_eq!(recognize_capability("Claude Code", None), AgentTier::Expert);
        assert_eq!(recognize_capability("opus-runner", None), AgentTier::Expert);
        assert_eq!(
            recognize_capability("claude-sonnet-4", None),
            AgentTier::Expert
        );
        // Skilled: codex / gpt / sonnet / gemini。
        assert_eq!(recognize_capability("Codex", None), AgentTier::Skilled);
        assert_eq!(recognize_capability("gpt-5", None), AgentTier::Skilled);
        assert_eq!(recognize_capability("Gemini", None), AgentTier::Skilled);
        // program 参数也参与匹配。
        assert_eq!(
            recognize_capability("worker", Some("SONNET-cli")),
            AgentTier::Skilled
        );
        // 其余落 Base。
        assert_eq!(recognize_capability("hermes", None), AgentTier::Base);
        assert_eq!(recognize_capability("", None), AgentTier::Base);
    }
}
