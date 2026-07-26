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

/// 扫描结果复用窗口。面板轮询 3s 一次，取略小的窗口让「新起的 agent」最迟一轮入册。
const TTL: Duration = Duration::from_millis(2500);

/// 沿进程树向下找 agent 的最大深度。shell → (npm/node/cmd 包装) → agent 本体，
/// 3 层足够覆盖 `claude` / `npx claude` / PowerShell 包装等常见形态。
const MAX_DEPTH: usize = 3;

/// 某 pane 上识别到的 agent。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneAgent {
    pub pane: Uuid,
    /// 进程名去扩展名后的 stem（`claude` / `codex` …），作展示名兜底。
    pub name: String,
    pub pid: u32,
}

/// 纯函数核心：给定 pane→shell pid 与一张 (pid, ppid, name) 进程表，
/// 找出每个 pane 下**最深的**一个 agent CLI 进程。注入进程表以便单测无 OS 耦合。
pub fn match_agent_panes(
    panes: &[(Uuid, u32)],
    procs: &[(u32, Option<u32>, String)],
) -> Vec<PaneAgent> {
    let mut children: HashMap<u32, Vec<(u32, &str)>> = HashMap::new();
    for (pid, ppid, name) in procs {
        if let Some(ppid) = ppid {
            children.entry(*ppid).or_default().push((*pid, name.as_str()));
        }
    }
    let mut out = Vec::new();
    for (pane, shell_pid) in panes {
        // BFS，取首个命中（更靠近 shell 的那个即 agent 本体或其包装，二者名字都
        // 会命中 KNOWN_AGENT_NAMES；包装层如 node/npx 不在名单里，自然跳过）。
        let mut frontier = vec![*shell_pid];
        'depth: for _ in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for pid in frontier.drain(..) {
                for (cpid, cname) in children.get(&pid).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if let Some(stem) = agent_stem(cname) {
                        out.push(PaneAgent {
                            pane: *pane,
                            name: stem,
                            pid: *cpid,
                        });
                        break 'depth;
                    }
                    next.push(*cpid);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
    }
    out
}

/// 进程名命中 agent CLI 名单则返回其 stem（去路径、去 `.exe`、小写）。
/// 名单与 [`super::discover::KNOWN_AGENT_NAMES`] 同源，避免两处漂移。
fn agent_stem(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&lower)
        .trim_end_matches(".exe");
    super::discover::KNOWN_AGENT_NAMES
        .iter()
        .any(|k| stem.contains(k))
        .then(|| stem.to_string())
}

/// TTL 缓存的真实扫描：一次进程表刷新 → 对本次传入的 pane 集合做匹配。
///
/// 缓存键含 pane 集合，pane 增删（split/close）会立即失效重扫，不会因为窗口内
/// 复用而漏掉新 pane。
pub fn scan_cached(panes: &[(Uuid, u32)]) -> Vec<PaneAgent> {
    type Cache = Option<(Instant, Vec<(Uuid, u32)>, Vec<PaneAgent>)>;
    static CACHE: Mutex<Cache> = Mutex::new(None);

    if panes.is_empty() {
        return Vec::new();
    }
    let mut key = panes.to_vec();
    key.sort();
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, cached_key, cached)) = guard.as_ref() {
        if at.elapsed() < TTL && cached_key == &key {
            return cached.clone();
        }
    }
    let found = match_agent_panes(panes, &list_processes());
    *guard = Some((Instant::now(), key, found.clone()));
    found
}

/// 进程内枚举 (pid, ppid, image name)。仅刷新进程表，不取 CPU/内存/exe 路径。
fn list_processes() -> Vec<(u32, Option<u32>, String)> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new().with_exe(UpdateKind::Never)),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| {
            (
                pid.as_u32(),
                p.parent().map(|pp| pp.as_u32()),
                p.name().to_string_lossy().to_string(),
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
}
