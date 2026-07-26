//! Domain Zero 端侧多智能体协同 —— 桌面 Tauri 命令面.
//!
//! 把 `ridge_core::teammate` 纯核心与运行态侧表桥接成前端可 `invoke` 的命令：
//! - [`get_teammate_topology`] —— D1 Agent Center 侧栏的花名册快照（只读）。
//! - [`resolve_hitl_request`] / [`set_hitl_enabled`] —— D2 HITL 网关裁决与开关。
//! - [`classify_command_risk`] —— 暴露 D2 风险分级器供 UI/调试查询。
//!
//! 拓扑快照从现有 `Workspace` 侧表（`teammate_agent_pane_map` / `_pane_states` /
//! `_pane_titles`）映射，pane 用真实 Uuid 字符串（非 core 内部的 u32）。这条回退路径
//! （无 typed 画像时）按 agent 名/标题**被动识别**能力档并跑极简组长竞选
//! （[`ridge_core::elect_leader`]），产出真实 `leaderId` + Leader 角色。

use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::state::{AppState, PaneState, Workspace};
use crate::teammate::hitl;

/// 把一个工作区的 teammate 侧表映射为前端 `TopologySnapshot` JSON。
/// `pub(crate)` 以便 teammate HTTP 路由 (`server.rs::route_get_team_profile`) 复用。
/// `wid` 供 G1 暂停侧表查询（suspended pane 的 status 投影为 `Suspended`）。
pub(crate) fn topology_json(ws: &Workspace, wid: Uuid) -> Value {
    // 叶子顺序 = MCP 数字索引寻址 (`teammate_pane_uuid_at_index`) 的同源序列。
    // 据此为每个成员补出数字 `paneIndex`，让 agent 读 `active-panes` 后既能回传
    // `paneId`(Uuid) 也能回传 `paneIndex`(数字)，两者都可寻址（缺口1 自洽）。
    let leaves = ws.pane_tree.get_all_leaves();

    // 能力档识别只认**稳定标识 agent_id**：pane 标题受 shell 控制、可随终端标题转义序列
    // 变动，若据它竞选组长会让 Leader 在轮询间跳变，也与 profiles 主路径（按注册身份定档）
    // 口径不一致（评审 #4）。这里改用 agent_id 定档——与 profiles 同源、稳定、不可被标题
    // 伪造；展示名 `name` 仍用 pane 标题。cap 每 agent 只识别一次，供竞选与 roster 复用。
    let cap_of = |agent_id: &str| ridge_core::recognize_capability(agent_id, None);
    let name_of = |agent_id: &str, pane: &Uuid| -> String {
        ws.teammate_pane_titles
            .get(pane)
            .cloned()
            .unwrap_or_else(|| agent_id.to_string())
    };
    // 组长不再由能力自动竞选（改由前端「编组」为每个组手动指定组长）；顶层花名册无全局
    // Leader，leaderId 恒 null、所有成员 role=Worker。`cap_of`/`name_of` 仍供 roster 用。
    let leader_id: Option<String> = None;

    let roster: Vec<Value> = ws
        .teammate_agent_pane_map
        .iter()
        .map(|(agent_id, pane)| {
            // G1：暂停覆写运行状态（Suspended > Working/Idle），无新字段。
            let status = if crate::teammate::suspend::is_suspended(wid, *pane) {
                "Suspended"
            } else {
                match ws.teammate_pane_states.get(pane) {
                    Some(PaneState::Busy) => "Working",
                    _ => "Idle",
                }
            };
            let name = name_of(agent_id, pane);
            let cap = cap_of(agent_id);
            let pane_index = leaves
                .iter()
                .position(|p| p == pane)
                .map(|i| json!(i))
                .unwrap_or(Value::Null);
            let role = "Worker";
            json!({
                "id": agent_id,
                "name": name,
                "paneId": pane.to_string(),
                "paneIndex": pane_index,
                "role": role,
                "status": status,
                "capability": serde_json::to_value(cap).unwrap_or(Value::Null),
            })
        })
        .collect();
    // 编组镜像（桌面双写落 workspace-memory）随快照下发，供 remote 只读同步。
    let groups = crate::teammate::memory::dir()
        .map(|d| crate::teammate::memory::get_teammate_groups(d, wid))
        .unwrap_or_else(|| json!([]));
    json!({
        "roster": roster,
        "leaderId": leader_id.map(Value::from).unwrap_or(Value::Null),
        "edges": [],
        "groups": groups,
    })
}

/// D1 —— 返回某工作区（缺省=活动工作区）的团队拓扑快照。只读。
#[tauri::command]
pub async fn get_teammate_topology(
    workspace_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let wid = match workspace_id {
        Some(s) => Uuid::parse_str(&s).map_err(|e| e.to_string())?,
        None => *state.active_workspace.read(),
    };
    // 有 typed 画像 → 跑 Leader 竞选（真实角色/leader）；否则回退侧表映射。
    // 两路都补 `paneIndex`：典型画像路径需把工作区当前叶子顺序传入 topology_for。
    let workspaces = state.workspaces.read();
    let ws = workspaces
        .get(&wid)
        .ok_or_else(|| format!("workspace {wid} not found"))?;
    let mut topo = if crate::teammate::profiles::has(wid) {
        let leaves = ws.pane_tree.get_all_leaves();
        crate::teammate::profiles::topology_for(wid, &leaves)
    } else {
        topology_json(ws, wid)
    };
    inject_roster_titles(&mut topo, ws);
    Ok(topo)
}

/// iter-60 G7 —— roster 条目并入 pane 标题（`title`，OSC 实时标题）：Commune MCP
/// （ridge_get_team_profile）与远端 roster 只读感知「队友正在跑什么」的轻量摘要，
/// 免去 PTY 尾行抓取/额外 LLM。两条拓扑路径（typed profiles / 侧表映射）共用。
pub fn inject_roster_titles(topology: &mut Value, ws: &crate::state::Workspace) {
    let Some(roster) = topology.get_mut("roster").and_then(|r| r.as_array_mut()) else {
        return;
    };
    for entry in roster {
        let Some(pid) = entry
            .get("paneId")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            continue;
        };
        // iter-61：优先取 PTY 解析器的实时 OSC 标题（与 pane 列表同源），
        // 侧表 teammate_pane_titles 仅作回退——前端据此把成员显示名同步到 pane 标题。
        let live = ws
            .terminals
            .get(&pid)
            .and_then(|h| h.parser.lock().title())
            .filter(|t| !t.trim().is_empty());
        let t = live.or_else(|| ws.teammate_pane_titles.get(&pid).cloned());
        if let Some(t) = t {
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("title".into(), json!(t));
            }
        }
    }
}

/// G1 —— 暂停（软门控 + 可选 OS 冻结）。
/// 优先读 `PtyHandle.child_pid` / `PtyHandle.job`（spawn 时挂上的 Job Object）；
/// `pty_pid` 仅作前端回退。OS 失败 fail-open。仅桌面本机 IPC，不入 `REMOTE_ALLOWLIST`。
#[tauri::command]
pub fn suspend_agent(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
    pty_pid: Option<u32>,
) -> Result<(), String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let pane = Uuid::parse_str(&pane_id).map_err(|e| e.to_string())?;
    let workspaces = state.workspaces.read();
    let handle = workspaces
        .get(&wid)
        .and_then(|ws| ws.terminals.get(&pane));
    let pid = handle.and_then(|h| h.child_pid).or(pty_pid);
    let job = handle.and_then(|h| h.job.as_ref());
    crate::teammate::suspend::suspend_with_os(wid, pane, pid, job, false)?;
    drop(workspaces);
    crate::teammate::suspend::persist_for(wid);
    Ok(())
}

/// G1 —— 恢复（含 OS thaw 若 suspend 时冻结成功）。幂等。
/// 若 pane 仍有 `PtyHandle.job`，经 job 入口 thaw；否则按 pid 直调 os_freeze（不 create_job）。
#[tauri::command]
pub fn resume_agent(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let pane = Uuid::parse_str(&pane_id).map_err(|e| e.to_string())?;
    let workspaces = state.workspaces.read();
    let job = workspaces
        .get(&wid)
        .and_then(|ws| ws.terminals.get(&pane))
        .and_then(|h| h.job.as_ref());
    crate::teammate::suspend::resume_with_job(wid, pane, job);
    drop(workspaces);
    crate::teammate::suspend::persist_for(wid);
    Ok(())
}

/// R17-TEAM-HEALTH —— 编排健康快照（suspended / pending HITL）。
#[tauri::command]
pub fn get_orchestration_health() -> Value {
    crate::teammate::orch_health::orchestration_health()
}

/// R17-HITL-BADGE —— pending 审批数量。
#[tauri::command]
pub fn get_pending_hitl_count() -> usize {
    crate::teammate::hitl::pending_count()
}

/// AC4-C7 —— 远端可读脱敏审批历史（无命令全文）。
#[tauri::command]
pub fn list_hitl_audit_remote(limit: Option<u32>) -> Value {
    crate::teammate::hitl_audit::list_audit_remote(limit.unwrap_or(20) as usize)
}

/// R17-CTX —— 扫描工作区根的 AGENTS.md / CLAUDE.md。
#[tauri::command]
pub fn scan_workspace_context_files(workspace_root: String) -> Result<Value, String> {
    let files =
        crate::teammate::context_files::scan_context_files(std::path::Path::new(&workspace_root));
    let block = crate::teammate::context_files::format_context_block(&files);
    Ok(json!({
        "files": files.iter().map(|f| json!({
            "name": f.name,
            "path": f.path.to_string_lossy(),
            "bytes": f.content.len(),
        })).collect::<Vec<_>>(),
        "promptBlock": block,
    }))
}

/// V-G1-RB —— 对 workspace root 做 git worktree 补丁快照，写入 sidecar rollbackPatches。
#[tauri::command]
pub fn checkpoint_workspace_rollback(
    workspace_id: String,
    workspace_root: String,
    label: Option<String>,
) -> Result<Value, String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let Some(dir) = crate::teammate::memory::dir() else {
        return Err("workspace-memory dir not initialized".into());
    };
    let patch = crate::teammate::rollback::checkpoint(
        dir,
        wid,
        std::path::Path::new(&workspace_root),
        label.unwrap_or_else(|| "manual".into()),
    )?;
    serde_json::to_value(patch).map_err(|e| e.to_string())
}

/// V-G1-RB —— 用最新 rollbackPatches 条目恢复工作树。
#[tauri::command]
pub fn rollback_workspace(
    workspace_id: String,
    workspace_root: String,
) -> Result<(), String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let Some(dir) = crate::teammate::memory::dir() else {
        return Err("workspace-memory dir not initialized".into());
    };
    let patch = crate::teammate::rollback::latest_patch(dir, wid)
        .ok_or_else(|| "no rollbackPatches in workspace memory".to_string())?;
    crate::teammate::rollback::rollback(std::path::Path::new(&workspace_root), &patch)
}

/// M1 切片三 —— 读 workspace memory 摘要（goal/constraints/tasks/…）。仅桌面 IPC。
#[tauri::command]
pub fn get_workspace_memory(workspace_id: String) -> Result<Value, String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let Some(dir) = crate::teammate::memory::dir() else {
        return Ok(json!({}));
    };
    Ok(crate::teammate::memory::read_summary(dir, wid))
}

/// M1 切片三 —— 写 goal / constraints / tasks（任一字段可选）。仅桌面 IPC。
/// 桌面「编组」→ 后端镜像：把前端编组定义（`TeammateGroup[]` JSON）写入 workspace-memory，
/// 供 remote 手机端经 `get_teammate_topology` 的 `groups` 字段只读同步。编组仍以桌面
/// localStorage 为权威真相；此命令是每次桌面编组变更后的 fire-forget 投影。
#[tauri::command]
pub fn set_teammate_groups(workspace_id: String, groups: Value) -> Result<(), String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let Some(dir) = crate::teammate::memory::dir() else {
        return Ok(());
    };
    crate::teammate::memory::set_teammate_groups(dir, wid, &groups);
    Ok(())
}

#[tauri::command]
pub fn set_workspace_memory(
    workspace_id: String,
    goal: Option<String>,
    constraints: Option<Vec<String>>,
    tasks: Option<Vec<Value>>,
) -> Result<(), String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let Some(dir) = crate::teammate::memory::dir() else {
        return Err("workspace-memory dir not initialized".into());
    };
    if let Some(g) = goal {
        crate::teammate::memory::set_goal(dir, wid, g);
    }
    if let Some(c) = constraints {
        crate::teammate::memory::set_constraints(dir, wid, c);
    }
    if let Some(t) = tasks {
        crate::teammate::memory::set_tasks(dir, wid, t);
    }
    Ok(())
}

/// V-DISC / iter-60 G6 —— 探测本机常见 agent CLI 进程（`enabled=false` 时恒空）。
///
/// 实现约束（对话需求「轻量化、性能要好」+ git 风暴 postmortem）：
/// - **进程内枚举**（sysinfo），不再 spawn `tasklist`/`ps` 子进程（原实现是一处
///   绕过外部进程闸的裸 spawn）；
/// - **5s TTL 缓存**：UI 轮询/多调用方共享一次扫描，关面板即零开销；
/// - 匹配逻辑复用 `teammate::discover::discover_agents` 纯函数（已有单测钉死）。
#[tauri::command]
pub fn discover_cli_agents(enabled: bool) -> Result<Value, String> {
    if !enabled {
        return Ok(Value::Array(vec![]));
    }
    Ok(Value::Array(
        discovered_agents_cached()
            .into_iter()
            .map(|a| json!({ "name": a.name, "pid": a.pid }))
            .collect(),
    ))
}

/// 5s TTL 缓存的进程指纹扫描（G6）。
fn discovered_agents_cached() -> Vec<crate::teammate::discover::DiscoveredAgent> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    type Cache = Option<(Instant, Vec<crate::teammate::discover::DiscoveredAgent>)>;
    static CACHE: Mutex<Cache> = Mutex::new(None);
    const TTL: Duration = Duration::from_secs(5);

    let mut guard = CACHE.lock().unwrap();
    if let Some((at, cached)) = guard.as_ref() {
        if at.elapsed() < TTL {
            return cached.clone();
        }
    }
    let procs = list_process_names_sysinfo();
    let found = crate::teammate::discover::discover_agents(
        true,
        &procs
            .iter()
            .map(|(pid, n)| (*pid, n.as_str()))
            .collect::<Vec<_>>(),
    );
    *guard = Some((Instant::now(), found.clone()));
    found
}

/// 进程内枚举 (pid, image name) —— 仅刷新进程表，不取 CPU/内存等重字段。
fn list_process_names_sysinfo() -> Vec<(u32, String)> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let sys = System::new_with_specifics(
        RefreshKind::new()
            .with_processes(ProcessRefreshKind::new().with_exe(UpdateKind::Never)),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| (pid.as_u32(), p.name().to_string_lossy().to_string()))
        .collect()
}

/// P2 阶段 1 —— 待裁决高危动作的**脱敏**只读列表（`teammate` 能力下远端可见）。
/// 投影仅 `{id, initiator, level, reason, createdAt}`——不含 `action` 命令全文
/// （可含密钥；见 `hitl::list_pending` 的钉死测试）。裁决通道仍不可远达。
#[tauri::command]
pub fn list_hitl_pending() -> Result<Value, String> {
    Ok(Value::Array(hitl::list_pending()))
}

/// P2 阶段 2 —— 远端裁决（`teammate` 能力下 mutating 方法）：一次性 nonce 票据
/// 恒时比对 + 单次消费；verdict 仅 approve/reject（modify 永不开放）。返回
/// `{outcome}` ∈ consumed/already-resolved/nonce-mismatch/bad-verdict。
#[tauri::command]
pub fn resolve_hitl_remote(id: String, nonce: String, verdict: String) -> Result<Value, String> {
    Ok(json!({ "outcome": hitl::resolve_remote(&id, &nonce, &verdict) }))
}

/// M1 切片二 —— 某工作区的裁决审计历史（环形 ≤50，最旧在前）。**仅桌面本机 IPC**，
/// 刻意不入 `REMOTE_ALLOWLIST`（远端暴露需宣告纪律，待需求）。条目无命令全文。
#[tauri::command]
pub fn list_hitl_decisions(workspace_id: String) -> Result<Value, String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let decisions = crate::teammate::memory::dir()
        .and_then(|d| crate::teammate::memory::read(d, wid))
        .and_then(|doc| doc.get("decisions").cloned())
        .unwrap_or_else(|| Value::Array(Vec::new()));
    Ok(decisions)
}

/// D2 —— 人类对一个挂起的高危动作的裁决回传。
/// `verdict` ∈ {"approve","reject","modify"}；modify 时 `replacement` 为新指令。
#[tauri::command]
pub fn resolve_hitl_request(
    id: String,
    verdict: String,
    replacement: Option<String>,
) -> Result<bool, String> {
    Ok(hitl::resolve(&id, &verdict, replacement))
}

/// D2 —— 开/关 HITL 审批网关（默认关，保持 send-keys 行为零变化）。
#[tauri::command]
pub fn set_hitl_enabled(enabled: bool) -> Result<(), String> {
    hitl::set_enabled(enabled);
    Ok(())
}

/// D2 —— 暴露风险分级器：把一条裸命令行分级为 {level, reason}。
#[tauri::command]
pub fn classify_command_risk(command: String) -> Result<Value, String> {
    serde_json::to_value(ridge_core::classify_shell_command(&command)).map_err(|e| e.to_string())
}

/// 功能2 —— 返回当前 teammate MCP 端点 + Bearer token，供指挥部「复制连接信息」按钮用。
/// 先惰性拉起 teammate server（与首个 PTY 注入同一路径），再读运行态 `teammate_binding`。
/// binding 为 None（服务尚未启动）时返回明确错误，前端据此提示「先打开一个终端分屏」。
///
/// **安全（设计文档 D6 硬约束）**：本命令返回**鉴权 token**，**仅限桌面本机 IPC 调用**——
/// 绝不加入 `REMOTE_ALLOWLIST`（`packages/ridge-core/src/capability.rs`），不暴露给
/// web-remote / LAN host / 云端控制面。否则任一远端控制器即可窃取本机 MCP 令牌、冒充队友。
/// token 只在运行时动态返回，绝不写入任何静态文档或仓库文件。
#[tauri::command]
pub fn get_teammate_connection_info(state: State<'_, AppState>) -> Result<Value, String> {
    crate::teammate::ensure_teammate_started(&state);
    let binding = state
        .teammate_binding
        .read()
        .clone()
        .ok_or_else(|| "teammate 服务未启动：请先打开一个终端分屏".to_string())?;
    // base_url 形如 http://127.0.0.1:<port>；只替换 scheme 前缀（replacen 限 1 次）。
    let ws_endpoint = format!(
        "{}/api/v1/mcp/ws",
        binding.base_url.replacen("http", "ws", 1)
    );
    Ok(json!({ "wsEndpoint": ws_endpoint, "token": binding.token }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{PaneState, TeammateMetrics, Workspace};
    use std::collections::{HashMap, HashSet};
    use std::time::SystemTime;

    fn ws_with_agent() -> Workspace {
        let mut ws = Workspace {
            pane_tree: crate::engine::pane_tree::PaneTree::new(),
            terminals: HashMap::new(),
            teammate_tmux_pane_cursor: 0,
            teammate_pane_titles: HashMap::new(),
            pane_sizes: HashMap::new(),
            last_pane_index: None,
            created_at: SystemTime::now(),
            teammate_pane_states: HashMap::new(),
            teammate_agent_pane_map: HashMap::new(),
            teammate_owned_panes: HashSet::new(),
            associated_file_path: None,
            pending_spawns: HashMap::new(),
            pty_generation: HashMap::new(),
            teammate_metrics: TeammateMetrics::default(),
            display_seq: 1,
        };
        let pane = Uuid::new_v4();
        ws.teammate_agent_pane_map.insert("claude-a".into(), pane);
        ws.teammate_pane_states.insert(pane, PaneState::Busy);
        ws.teammate_pane_titles.insert(pane, "编译中".into());
        ws
    }

    /// P1/S1 脱敏门禁：远程暴露的拓扑投影不得含敏感字段（get_teammate_topology 自
    /// iteration 6 起进 REMOTE_ALLOWLIST，此投影即远端可见面）。
    #[test]
    fn topology_projection_has_no_sensitive_fields() {
        let ws = ws_with_agent();
        let v = topology_json(&ws, Uuid::new_v4());
        let json = v.to_string().to_lowercase();
        for needle in ["token", "endpoint", "env_", "secret", "seed", "mcp"] {
            assert!(!json.contains(needle), "topology projection leaks `{needle}`: {json}");
        }
        let member = v["roster"][0].as_object().expect("roster member object");
        for key in member.keys() {
            assert!(
                ["id", "name", "paneId", "paneIndex", "role", "status", "capability"]
                    .contains(&key.as_str()),
                "unexpected roster field `{key}`"
            );
        }
    }

    /// G1：暂停覆写 status 投影为 Suspended（无新字段），恢复后回落运行态。
    #[test]
    fn topology_status_reflects_suspension() {
        let ws = ws_with_agent();
        let wid = Uuid::new_v4();
        let pane = *ws.teammate_agent_pane_map.values().next().unwrap();

        assert_eq!(topology_json(&ws, wid)["roster"][0]["status"], "Working");
        crate::teammate::suspend::suspend(wid, pane);
        let v = topology_json(&ws, wid);
        assert_eq!(v["roster"][0]["status"], "Suspended");
        // 字段集不因暂停扩张（脱敏面不变）。
        assert_eq!(v["roster"][0].as_object().unwrap().len(), 7);
        crate::teammate::suspend::resume(wid, pane);
        assert_eq!(topology_json(&ws, wid)["roster"][0]["status"], "Working");
    }
}
