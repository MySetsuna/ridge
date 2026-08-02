//! Registry of external binary spawn sites (AC4 volume · process_guard adoption).
//!
//! Documents and enforces that production spawns go through guarded exits:
//! - git → `commands::git` → `process_guard::kill_process_tree` on timeout
//! - future helpers should register here and use `process_guard::run_command_with_timeout`
//!
//! This is not a full dynamic interceptor; it is a **contract + testable catalog**
//! so new `Command::new` sites cannot claim they are outside the guard policy.

/// Known production external binaries that may be spawned by ridge-core / hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalBinary {
    Git,
    Taskkill,
    Kill,
    /// Placeholder for future helpers (rg, node tools, etc.)
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnSite {
    pub binary: ExternalBinary,
    /// Module path documentation (for audits).
    pub module: &'static str,
    /// Must use process_guard timeout kill path.
    pub requires_tree_kill_on_timeout: bool,
}

/// Catalog of known sites. Tests assert policy invariants.
pub const SPAWN_SITES: &[SpawnSite] = &[
    SpawnSite {
        binary: ExternalBinary::Git,
        module: "commands::git",
        requires_tree_kill_on_timeout: true,
    },
    SpawnSite {
        binary: ExternalBinary::Taskkill,
        module: "process_guard::kill_process_tree",
        requires_tree_kill_on_timeout: false,
    },
    SpawnSite {
        binary: ExternalBinary::Kill,
        module: "process_guard::kill_process_tree",
        requires_tree_kill_on_timeout: false,
    },
];

pub fn sites_requiring_tree_kill() -> Vec<&'static SpawnSite> {
    SPAWN_SITES
        .iter()
        .filter(|s| s.requires_tree_kill_on_timeout)
        .collect()
}

pub fn all_tree_kill_sites_covered_by_process_guard() -> bool {
    sites_requiring_tree_kill()
        .iter()
        .all(|s| s.module.contains("git") || s.module.contains("process_guard"))
}

/// Policy: git is the only app-level binary that must timeout-kill today.
pub fn primary_timeout_binary() -> ExternalBinary {
    ExternalBinary::Git
}

/// Human-readable policy lines for diagnostics / UI.
pub fn policy_lines() -> Vec<String> {
    SPAWN_SITES
        .iter()
        .map(|s| {
            format!(
                "{:?} @ {} tree_kill_on_timeout={}",
                s.binary, s.module, s.requires_tree_kill_on_timeout
            )
        })
        .collect()
}

/// Register a future helper site (compile-time catalog is authoritative;
/// this validates naming for docs/tests).
pub fn validate_site_module(module: &str) -> bool {
    !module.is_empty()
        && module
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_' || c == '-')
}

/// Cross-layer concurrency caps (must match frontend processGuardPolicy/pLimit
/// and `commands::git`). Keep these bounds identical so telemetry and clamps do
/// not report a different guard from the semaphore that actually runs git.
pub const GIT_CONCURRENCY_MIN: u32 = 2;
pub const GIT_CONCURRENCY_MAX: u32 = 12;

pub fn clamp_git_concurrency(n: u32) -> u32 {
    n.clamp(GIT_CONCURRENCY_MIN, GIT_CONCURRENCY_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_guard::{process_guard_stats, run_command_with_timeout};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    fn rust_files_below(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("target" | ".git" | "node_modules")
                ) {
                    rust_files_below(&path, files);
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    #[test]
    fn catalog_has_git_requiring_tree_kill() {
        let kills = sites_requiring_tree_kill();
        assert!(kills.iter().any(|s| s.binary == ExternalBinary::Git));
        assert!(all_tree_kill_sites_covered_by_process_guard());
        assert_eq!(primary_timeout_binary(), ExternalBinary::Git);
    }

    #[test]
    fn process_guard_is_the_timeout_exit() {
        // Smoke: process_guard runs a quick command (same exit used by git).
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/C", "echo ok"]);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("echo");
            c.arg("ok");
            c
        };
        let out = run_command_with_timeout(&mut cmd, Duration::from_secs(5)).expect("echo");
        assert!(out.status.success());
        let _ = process_guard_stats();
    }

    #[test]
    fn policy_and_concurrency_caps() {
        assert!(!policy_lines().is_empty());
        assert!(validate_site_module("commands::git"));
        assert!(!validate_site_module(""));
        assert_eq!(clamp_git_concurrency(0), GIT_CONCURRENCY_MIN);
        assert_eq!(clamp_git_concurrency(99), GIT_CONCURRENCY_MAX);
        assert_eq!(clamp_git_concurrency(2), 2);
    }

    #[test]
    fn workspace_has_no_git_spawn_outside_shared_gate() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let gate = workspace.join("packages/ridge-core/src/commands/git.rs");
        let this_file = workspace.join("packages/ridge-core/src/external_spawn_registry.rs");
        let mut files = Vec::new();
        rust_files_below(&workspace.join("packages"), &mut files);
        rust_files_below(&workspace.join("src-tauri"), &mut files);
        let mut offenders = Vec::new();
        for file in files {
            if file == gate || file == this_file {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (line, text) in source.lines().enumerate() {
                if text.contains("Command::new(\"git\")") {
                    offenders.push(format!(
                        "{}:{}",
                        file.strip_prefix(workspace).unwrap_or(&file).display(),
                        line + 1
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "git spawns must use commands::git::run_git_guarded: {offenders:?}"
        );
    }
}
