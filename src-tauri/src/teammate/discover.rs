//! Agent CLI 进程名单（SSOT）。
//!
//! iter-62：原来的「本机全量进程指纹发现」已退场——它会把 Ridge **外面**跑的
//! claude / codex 也算进花名册，与用户「只要在 Ridge 里运行的 agent」的口径相悖。
//! 判据改为「某 pane 的 shell 子树里挂着 agent CLI」，实现见
//! [`super::autodiscover`]；本模块只留下**名单本身**，供其匹配。

/// Known agent CLI process name substrings (case-insensitive match on image name).
pub const KNOWN_AGENT_NAMES: &[&str] = &[
    "claude",
    "claude-code",
    "codex",
    "cursor-agent",
    "gemini",
    "aider",
    "continue",
];
