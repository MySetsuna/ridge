//! 「在 Ridge 里跑着的 agent」自动识别（iter-62）。
//!
//! 用户需求：成员名册**不该**靠人手点「标记为 agent」，也不该是本机全量进程指纹
//! （那会把 Ridge 外面跑的 claude/codex 也算进来）。判据只有一条——
//! **某个 pane 的 shell 底下真的挂着一个 agent CLI 进程**。
//!
//! 性能是硬约束（见 `docs/iterations/2026-07-24-git-pileup-postmortem.md`）：
//! - 一次工作区扫描**只刷一次进程表**。逐 pane 调
//!   `ridge_core::commands::process::get_foreground_process_name` 会 N 次
//!   `refresh_processes(All)` —— N 个 pane 就是 N 次全表扫描，正是要避免的形态。
//! - 结果按 TTL 缓存，面板 3s 轮询与远端拉取共享同一次扫描。
//! - 关闭自动识别开关时零扫描。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

/// 扫描结果复用窗口。事件触发刷新时短暂复用进程树，避免同一批事件重复扫描。
const TTL: Duration = Duration::from_millis(500);

/// 沿进程树向下找 agent 的最大深度，覆盖 shell → 包装器 → agent 本体。
const MAX_DEPTH: usize = 6;

type ScanCache = Option<(Instant, Vec<(Uuid, u32)>, Vec<PaneAgent>)>;
/// 与 `scan_cached` / `invalidate_cache` 共享，覆盖 processNames 后必须清空。
static SCAN_CACHE: Mutex<ScanCache> = Mutex::new(None);

/// 某 pane 上识别到的 agent。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneAgent {
    pub pane: Uuid,
    /// 进程名去扩展名后的 stem（`claude` / `codex` …），作展示名兜底。
    pub name: String,
    pub pid: u32,
    pub cwd: Option<String>,
    pub session_id: Option<String>,
}

/// 运行时入口：内置 + 磁盘/内存用户覆盖（[`super::agent_catalog::load_profile_overrides`]）。
pub fn match_agent_panes(
    panes: &[(Uuid, u32)],
    procs: &[(u32, Option<u32>, String)],
) -> Vec<PaneAgent> {
    let overrides = super::agent_catalog::load_profile_overrides();
    let names = super::discover::known_agent_names_runtime(&overrides);
    match_agent_panes_with_names(panes, procs, &names)
}

/// 运行时进程快照亦携 argv。Windows npm shim 常以
/// `node.exe .../@openai/codex/bin/codex.js` 启动 Codex；只看映像名会漏识别。
fn match_agent_panes_with_commands(
    panes: &[(Uuid, u32)],
    procs: &[(u32, Option<u32>, String, Vec<String>, Option<String>)],
    process_names: &[String],
) -> Vec<PaneAgent> {
    let normalized = procs
        .iter()
        .map(|(pid, ppid, image, argv, _)| {
            let identity = agent_stem(image, process_names)
                .or_else(|| agent_from_command(argv, process_names))
                .unwrap_or_else(|| image.clone());
            (*pid, *ppid, identity)
        })
        .collect::<Vec<_>>();
    let mut found = match_agent_panes_with_names(panes, &normalized, process_names);
    for agent in &mut found {
        if let Some((_, _, _, argv, cwd)) = procs.iter().find(|(pid, _, _, _, _)| *pid == agent.pid)
        {
            agent.cwd = cwd.clone();
            agent.session_id = agent_session_id_from_command(argv);
        }
    }
    found
}

/// 纯函数核心：给定 pane→shell pid、进程表、进程名单，找出每个 pane 下命中的 agent。
/// 单测注入自定义 processNames 时走此入口，证明覆盖名单参与识别。
pub fn match_agent_panes_with_names(
    panes: &[(Uuid, u32)],
    procs: &[(u32, Option<u32>, String)],
    process_names: &[String],
) -> Vec<PaneAgent> {
    let mut children: HashMap<u32, Vec<(u32, &str)>> = HashMap::new();
    for (pid, ppid, name) in procs {
        if let Some(ppid) = ppid {
            children
                .entry(*ppid)
                .or_default()
                .push((*pid, name.as_str()));
        }
    }
    let mut out = Vec::new();
    for (pane, shell_pid) in panes {
        if let Some((name, pid)) = find_agent(&children, *shell_pid, process_names) {
            out.push(PaneAgent {
                pane: *pane,
                name,
                pid,
                cwd: None,
                session_id: None,
            });
        }
    }
    out
}

fn find_agent(
    children: &HashMap<u32, Vec<(u32, &str)>>,
    shell_pid: u32,
    process_names: &[String],
) -> Option<(String, u32)> {
    // BFS，取首个命中（更靠近 shell 的那个即 agent 本体或其包装）。
    let mut frontier = vec![shell_pid];
    for _ in 0..MAX_DEPTH {
        let mut next = Vec::new();
        for pid in frontier.drain(..) {
            for (child_pid, child_name) in children.get(&pid).map(Vec::as_slice).unwrap_or(&[]) {
                if let Some(stem) = agent_stem(child_name, process_names) {
                    return Some((stem, *child_pid));
                }
                next.push(*child_pid);
            }
        }
        if next.is_empty() {
            return None;
        }
        frontier = next;
    }
    None
}

/// 进程名命中 agent CLI 名单则返回其 stem（去路径、去 `.exe`、小写）。
fn agent_stem(name: &str, process_names: &[String]) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let stem = [".exe", ".cmd", ".bat", ".ps1", ".js", ".cjs", ".mjs"]
        .iter()
        .find_map(|extension| stem.strip_suffix(extension))
        .unwrap_or(stem);
    process_names
        .iter()
        .any(|k| stem.contains(k.as_str()))
        .then(|| stem.to_string())
}

fn agent_from_command(argv: &[String], process_names: &[String]) -> Option<String> {
    process_names.iter().find_map(|candidate| {
        let candidate = candidate.to_ascii_lowercase();
        argv.iter()
            .any(|arg| command_arg_matches_agent(arg, &candidate))
            .then_some(candidate)
    })
}

fn command_arg_matches_agent(arg: &str, candidate: &str) -> bool {
    let normalized = arg.to_ascii_lowercase().replace('\\', "/");
    let tokens = normalized
        .split_whitespace()
        .map(|token| token.trim_matches(['"', '\'', '(', ')', '[', ']', ',', ';']))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.iter().any(|token| {
        let name = token.rsplit('/').next().unwrap_or(token);
        let stem = [".exe", ".cmd", ".bat", ".ps1", ".js", ".cjs", ".mjs"]
            .iter()
            .find_map(|extension| name.strip_suffix(extension))
            .unwrap_or(name);
        stem == candidate && name != candidate
    }) {
        return true;
    }
    // Official npm launcher: node_modules/@openai/codex/bin/codex.js.
    if candidate == "codex" && tokens.iter().any(|token| token.contains("/@openai/codex/")) {
        return true;
    }
    // Package managers commonly invoke a bare `codex` after `exec`; a bare
    // argument from `git commit -m codex` must remain unrelated.
    let launcher = tokens.iter().any(|token| {
        matches!(
            token.rsplit('/').next().unwrap_or(token),
            "node"
                | "node.exe"
                | "npm"
                | "npm.cmd"
                | "npx"
                | "npx.cmd"
                | "pnpm"
                | "pnpm.cmd"
                | "yarn"
                | "yarn.cmd"
                | "cmd"
                | "cmd.exe"
                | "pwsh"
                | "pwsh.exe"
                | "powershell"
                | "powershell.exe"
        )
    });
    launcher && tokens.iter().any(|token| *token == candidate)
}

fn agent_session_id_from_command(argv: &[String]) -> Option<String> {
    for (index, raw) in argv.iter().enumerate() {
        let token = raw.trim_matches(['"', '\'', '(', ')', '[', ']', ',', ';']);
        let lower = token.to_ascii_lowercase();
        if let Some(value) = lower
            .strip_prefix("--resume=")
            .or_else(|| lower.strip_prefix("--session-id="))
        {
            if !value.is_empty() {
                return Some(token[token.len() - value.len()..].to_string());
            }
        }
        if matches!(lower.as_str(), "resume" | "--resume" | "--session-id") {
            if let Some(next) = argv.get(index + 1) {
                let value = next.trim_matches(['"', '\'', '(', ')', '[', ']', ',', ';']);
                if !value.is_empty() && !value.starts_with('-') {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// TTL 缓存的真实扫描：一次进程表刷新 → 对本次传入的 pane 集合做匹配。
///
/// 缓存键含 pane 集合，pane 增删（split/close）会立即失效重扫，不会因为窗口内
/// 复用而漏掉新 pane。
pub fn scan_cached(panes: &[(Uuid, u32)]) -> Vec<PaneAgent> {
    if panes.is_empty() {
        return Vec::new();
    }
    let mut key = panes.to_vec();
    key.sort();
    let mut guard = SCAN_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, cached_key, cached)) = guard.as_ref() {
        if at.elapsed() < TTL && cached_key == &key {
            return cached.clone();
        }
    }
    let overrides = super::agent_catalog::load_profile_overrides();
    let names = super::discover::known_agent_names_runtime(&overrides);
    let found = match_agent_panes_with_commands(panes, &list_processes(), &names);
    *guard = Some((Instant::now(), key, found.clone()));
    found
}

/// 设置覆盖变更后丢弃 TTL，下一轮轮询立刻按新 processNames 识别。
pub fn invalidate_cache() {
    let mut guard = SCAN_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// 进程内枚举 (pid, ppid, image name, argv, cwd)。仅刷新进程表、命令行与 cwd，
/// 不取 CPU/内存/exe 路径；同一轮仍只建一份全局快照。
fn list_processes() -> Vec<(u32, Option<u32>, String, Vec<String>, Option<String>)> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(
            ProcessRefreshKind::new()
                .with_exe(UpdateKind::Never)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_cwd(UpdateKind::OnlyIfNotSet),
        ),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| {
            (
                pid.as_u32(),
                p.parent().map(|pp| pp.as_u32()),
                p.name().to_string_lossy().to_string(),
                p.cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy().to_string())
                    .collect(),
                p.cwd().map(|path| path.to_string_lossy().into_owned()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws_pane(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn direct_child_agent_is_matched() {
        let pane = ws_pane(1);
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (200, Some(100), "claude.exe".to_string()),
        ];
        let found = match_agent_panes(&[(pane, 100)], &procs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "claude");
        assert_eq!(found[0].pid, 200);
    }

    #[test]
    fn agent_behind_a_wrapper_is_matched() {
        let pane = ws_pane(2);
        let procs = vec![
            (100, None, "bash".to_string()),
            (150, Some(100), "node".to_string()),
            (200, Some(150), "codex".to_string()),
        ];
        let found = match_agent_panes(&[(pane, 100)], &procs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "codex");
    }

    #[test]
    fn codex_npm_node_launcher_is_matched() {
        let pane = ws_pane(10);
        let procs = vec![
            (100, None, "pwsh.exe".to_string(), vec![], None),
            (150, Some(100), "cmd.exe".to_string(), vec![], None),
            (
                200,
                Some(150),
                "node.exe".to_string(),
                vec![
                    r"C:\DevKit\nodejs\node.exe".into(),
                    r"C:\DevKit\nodejs\node_modules\@openai\codex\bin\codex.js".into(),
                ],
                Some(r"C:\code\wind".into()),
            ),
        ];
        let names = super::super::discover::known_agent_names_runtime(&[]);
        let found = match_agent_panes_with_commands(&[(pane, 100)], &procs, &names);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "codex");
        assert_eq!(found[0].pid, 200);
        assert_eq!(found[0].cwd.as_deref(), Some(r"C:\code\wind"));
    }

    #[test]
    fn codex_resume_argument_is_retained_for_history_binding() {
        let pane = ws_pane(13);
        let procs = vec![
            (100, None, "pwsh.exe".to_string(), vec![], None),
            (
                200,
                Some(100),
                "codex.exe".to_string(),
                vec!["codex".into(), "resume".into(), "codex-native-1".into()],
                Some(r"C:\code\wind".into()),
            ),
        ];
        let names = super::super::discover::known_agent_names_runtime(&[]);
        let found = match_agent_panes_with_commands(&[(pane, 100)], &procs, &names);
        assert_eq!(found[0].session_id.as_deref(), Some("codex-native-1"));
    }

    #[test]
    fn agent_nested_behind_common_launchers_is_still_matched() {
        let pane = ws_pane(14);
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (110, Some(100), "cmd.exe".to_string()),
            (120, Some(110), "node.exe".to_string()),
            (130, Some(120), "npm.cmd".to_string()),
            (140, Some(130), "codex.cmd".to_string()),
        ];
        let found = match_agent_panes(&[(pane, 100)], &procs);
        assert_eq!(found[0].name, "codex");
    }

    #[test]
    fn unrelated_node_command_in_codex_named_workspace_is_not_matched() {
        let pane = ws_pane(11);
        let procs = vec![
            (100, None, "pwsh.exe".to_string(), vec![], None),
            (
                200,
                Some(100),
                "node.exe".to_string(),
                vec![
                    r"C:\DevKit\nodejs\node.exe".into(),
                    r"C:\code\codex\scripts\serve.js".into(),
                ],
                Some(r"C:\code\codex".into()),
            ),
        ];
        let names = super::super::discover::known_agent_names_runtime(&[]);
        assert!(match_agent_panes_with_commands(&[(pane, 100)], &procs, &names).is_empty());
    }

    #[test]
    fn codex_as_an_unrelated_argument_is_not_matched() {
        let pane = ws_pane(12);
        let procs = vec![
            (100, None, "pwsh.exe".to_string(), vec![], None),
            (
                200,
                Some(100),
                "git.exe".to_string(),
                vec![
                    "git.exe".into(),
                    "commit".into(),
                    "-m".into(),
                    "codex".into(),
                ],
                Some(r"C:\code\wind".into()),
            ),
        ];
        let names = super::super::discover::known_agent_names_runtime(&[]);
        assert!(match_agent_panes_with_commands(&[(pane, 100)], &procs, &names).is_empty());
    }

    #[test]
    fn plain_shell_yields_nothing() {
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (200, Some(100), "git.exe".to_string()),
        ];
        assert!(match_agent_panes(&[(ws_pane(3), 100)], &procs).is_empty());
    }

    /// 关键隔离：Ridge **外面**跑的 agent（不在任一 pane 的 shell 子树里）不得入册。
    /// 这正是用户「只要在 ridge 中运行的 agent 成员」的判据。
    #[test]
    fn agent_outside_any_pane_subtree_is_ignored() {
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (900, None, "claude.exe".to_string()), // 另一个终端里的 agent
        ];
        assert!(match_agent_panes(&[(ws_pane(4), 100)], &procs).is_empty());
    }

    #[test]
    fn each_pane_reports_its_own_agent() {
        let (a, b) = (ws_pane(5), ws_pane(6));
        let procs = vec![
            (10, None, "pwsh.exe".to_string()),
            (11, Some(10), "claude.exe".to_string()),
            (20, None, "pwsh.exe".to_string()),
            (21, Some(20), "gemini".to_string()),
        ];
        let found = match_agent_panes(&[(a, 10), (b, 20)], &procs);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|f| f.pane == a && f.name == "claude"));
        assert!(found.iter().any(|f| f.pane == b && f.name == "gemini"));
    }

    /// 0.1.5 回归：grok 必须经 agent_catalog 运行时名单命中。
    #[test]
    fn grok_process_under_pane_shell_is_matched() {
        let pane = ws_pane(7);
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (200, Some(100), "grok.exe".to_string()),
        ];
        let found = match_agent_panes(&[(pane, 100)], &procs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "grok");
        assert_eq!(found[0].pid, 200);
    }

    /// 自定义 processName 仅当注入名单时命中——证明识别走名单而非写死 builtins。
    #[test]
    fn custom_process_name_matched_only_when_in_names() {
        let pane = ws_pane(8);
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (200, Some(100), "my-custom-agent.exe".to_string()),
        ];
        let builtins = super::super::discover::known_agent_names_runtime(&[]);
        assert!(
            match_agent_panes_with_names(&[(pane, 100)], &procs, &builtins).is_empty(),
            "unknown stem must not match builtins-only list"
        );
        let mut with_custom = builtins;
        with_custom.push("my-custom-agent".into());
        let found = match_agent_panes_with_names(&[(pane, 100)], &procs, &with_custom);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "my-custom-agent");
        assert_eq!(found[0].pid, 200);
    }

    /// 运行时覆盖 store → `match_agent_panes` 实路径（非 with_names 旁路）。
    #[test]
    fn match_agent_panes_honors_in_memory_overrides() {
        use super::super::agent_catalog::{set_profile_overrides_in_memory, AgentProfile};
        let pane = ws_pane(9);
        let procs = vec![
            (100, None, "pwsh.exe".to_string()),
            (200, Some(100), "ridge-extra-cli.exe".to_string()),
        ];
        let prev = super::super::agent_catalog::load_profile_overrides();
        set_profile_overrides_in_memory(vec![AgentProfile {
            id: "ridge-extra".into(),
            process_names: vec!["ridge-extra-cli".into()],
            executable: "ridge-extra-cli".into(),
            resume_argv: vec![],
            yolo_args: vec![],
            yolo_position: "before".into(),
        }]);
        let found = match_agent_panes(&[(pane, 100)], &procs);
        set_profile_overrides_in_memory(prev);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "ridge-extra-cli");
    }
}
