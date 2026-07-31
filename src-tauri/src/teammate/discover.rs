//! Agent CLI 进程名单。
//!
//! 名单权威已迁到 [`super::agent_catalog`]（内置默认 + 用户覆盖）。
//! 本模块保留 `KNOWN_AGENT_NAMES` 静态切片以兼容旧调用点；运行时匹配请用
//! [`super::agent_catalog::known_process_names`]。

use super::agent_catalog;

/// 内置默认进程名（编译期常量，单测与无配置路径用）。
/// **含 grok**——0.1.5 回归：未列入则 pane 子树中的 grok 永不入册。
/// 运行时以 [`known_agent_names_runtime`] 为准；此常量须与 catalog 内置对齐。
pub const KNOWN_AGENT_NAMES: &[&str] = &[
    "claude",
    "claude-code",
    "codex",
    "grok",
    "cursor-agent",
    "gemini",
    "aider",
    "continue",
];

/// 运行时名单（可与用户设置合并）。autodiscover / 设置面板 / 测试均走此入口。
pub fn known_agent_names_runtime(overrides: &[agent_catalog::AgentProfile]) -> Vec<String> {
    let mut names = agent_catalog::known_process_names(overrides);
    // 保证静态默认表每一项都可被匹配（防 catalog 漏项）。
    for k in KNOWN_AGENT_NAMES {
        let k = (*k).to_string();
        if !names.iter().any(|n| n == &k) {
            names.push(k);
        }
    }
    names
}
