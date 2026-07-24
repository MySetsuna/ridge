//! Optional CLI agent process discovery (V-DISC). Default off at call sites.
//! Pure scan over an injected process name list — no OS coupling in unit tests.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredAgent {
    pub name: String,
    pub pid: u32,
}

/// Scan `procs` of `(pid, image_name)` when `enabled`. Empty when disabled.
pub fn discover_agents(enabled: bool, procs: &[(u32, &str)]) -> Vec<DiscoveredAgent> {
    if !enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (pid, name) in procs {
        let lower = name.to_ascii_lowercase();
        let stem = lower
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&lower)
            .trim_end_matches(".exe");
        if KNOWN_AGENT_NAMES.iter().any(|k| stem.contains(k)) {
            out.push(DiscoveredAgent {
                name: stem.to_string(),
                pid: *pid,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_empty() {
        let procs = [(1u32, "claude.exe"), (2, "codex")];
        assert!(discover_agents(false, &procs).is_empty());
    }

    #[test]
    fn enabled_matches_known_names() {
        let procs = [
            (10u32, "claude.exe"),
            (11, "notepad.exe"),
            (12, "C:\\tools\\codex.exe"),
            (13, "chrome"),
        ];
        let found = discover_agents(true, &procs);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|a| a.pid == 10 && a.name.contains("claude")));
        assert!(found.iter().any(|a| a.pid == 12 && a.name.contains("codex")));
    }
}
