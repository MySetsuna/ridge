//! 桌面宿主的 MCP 实装：把 `ridge_mcp::server::McpHost` 落到 `AppState`。
//!
//! 协议分发、工具语义、WS/HTTP 传输都在 `ridge-mcp` crate 里，与 rdg 无头 host
//! 共用同一份；本文件只回答「怎么动桌面工作区的 pane」。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use tauri::Emitter;
use uuid::Uuid;

use ridge_mcp::addressing::{parse_pane_target, PaneTarget};
use ridge_mcp::delivery::{
    DeliveryOutcome, DeliveryProbe, HubDeliveryAdapter, HubPtyRuntimeSnapshot, HubPtySafety,
};
use ridge_mcp::resource::{git_branch, git_root, RidgeUri};
use ridge_mcp::server::{
    ExternalExecutionRejection, HostError, HostResult, InputDispatch, LaunchCapabilities,
    LaunchProfile, McpHost, SplitPaneRequest,
};
use ridge_mcp::transport::{mcp_router, McpTransportCtx};

use super::layout_event::{LayoutChange, TEAMMATE_GROUP_ADD_MEMBER, TEAMMATE_LAYOUT_CHANGED};
use super::server::{team_profile_snapshot, TeammateCtx};
use crate::commands::{pane, terminal};
use crate::state::PaneState;

/// 把共享的 MCP 路由（`/api/v1/mcp/ws` + `POST /api/v1/mcp`）接到桌面工作区上。
pub(crate) fn router(ctx: TeammateCtx) -> axum::Router {
    let token = ctx.token.clone();
    mcp_router(McpTransportCtx::with_state(
        Arc::new(DesktopMcpHost { ctx }),
        token,
        env!("CARGO_PKG_VERSION"),
        desktop_hub_state(),
    ))
}

/// Native A2A boundary backed by the desktop's persistent Hub and the same
/// fenced host adapter used by MCP. The Agent Card is public; JSON-RPC is
/// authenticated by the persisted teammate token.
pub(crate) fn a2a_router(ctx: TeammateCtx) -> axum::Router {
    let token = ctx.token.clone();
    let host = Arc::new(DesktopMcpHost { ctx });
    ridge_mcp::native_a2a::router(ridge_mcp::native_a2a::NativeA2aCtx::new(
        host,
        token,
        env!("CARGO_PKG_VERSION"),
        desktop_hub_state(),
    ))
}

// ─── 宿主实装 ────────────────────────────────────────────────────────────────

struct DesktopMcpHost {
    ctx: TeammateCtx,
}

#[derive(Clone)]
struct DesktopPtyRuntimeRecord {
    agent_id: String,
    generation: u64,
    lease: String,
    snapshot: HubPtyRuntimeSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PtyRuntimeIdentity {
    pub agent_id: String,
    pub generation: u64,
    pub lease: String,
}

static DESKTOP_PTY_RUNTIME: OnceLock<Mutex<HashMap<(Uuid, Uuid), DesktopPtyRuntimeRecord>>> =
    OnceLock::new();

fn desktop_pty_runtime() -> &'static Mutex<HashMap<(Uuid, Uuid), DesktopPtyRuntimeRecord>> {
    DESKTOP_PTY_RUNTIME.get_or_init(|| Mutex::new(HashMap::new()))
}

fn value_string(args: &Value, camel: &str, snake: &str) -> Result<String, String> {
    args.get(camel)
        .or_else(|| args.get(snake))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{camel} must be a non-empty string"))
}

fn value_uuid(args: &Value, camel: &str, snake: &str) -> Result<Uuid, String> {
    let raw = value_string(args, camel, snake)?;
    Uuid::parse_str(&raw).map_err(|_| format!("{camel} must be a UUID"))
}

fn value_u64(args: &Value, camel: &str, snake: &str) -> Result<u64, String> {
    let value = args
        .get(camel)
        .or_else(|| args.get(snake))
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{camel} must be an unsigned integer"))?;
    (value != 0)
        .then_some(value)
        .ok_or_else(|| format!("{camel} must be non-zero"))
}

fn value_bool(args: &Value, camel: &str, snake: &str) -> Result<bool, String> {
    args.get(camel)
        .or_else(|| args.get(snake))
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{camel} must be a boolean"))
}

fn parse_pty_runtime_request(
    args: &Value,
) -> Result<(Uuid, Uuid, DesktopPtyRuntimeRecord), String> {
    let workspace_id = value_uuid(args, "workspaceId", "workspace_id")?;
    let pane_id = value_uuid(args, "paneId", "pane_id")?;
    let agent_id = value_string(args, "agentId", "agent_id")?;
    let generation = value_u64(args, "generation", "generation")?;
    let lease = value_string(args, "lease", "lease")?;
    let snapshot = HubPtyRuntimeSnapshot::new(
        HubPtySafety {
            agent_idle: value_bool(args, "agentIdle", "agent_idle")?,
            terminal_mode_agent_prompt: value_bool(
                args,
                "terminalModeAgentPrompt",
                "terminal_mode_agent_prompt",
            )?,
            pending_approval: value_bool(args, "pendingApproval", "pending_approval")?,
            foreground_is_target_agent: value_bool(
                args,
                "foregroundIsTargetAgent",
                "foreground_is_target_agent",
            )?,
            user_input_competing: value_bool(args, "userInputCompeting", "user_input_competing")?,
        },
        value_u64(args, "stateRevision", "state_revision")?,
        value_u64(args, "inputEpoch", "input_epoch")?,
    );
    Ok((
        workspace_id,
        pane_id,
        DesktopPtyRuntimeRecord {
            agent_id,
            generation,
            lease,
            snapshot,
        },
    ))
}

/// Resolve the pane's current fencing identity from the Kernel roster. The
/// desktop workspace map proves pane ownership; Kernel identity proves that
/// the generation/lease belongs to the currently running Agent session.
fn kernel_identity_for_pane(
    identities: &[ridge_core::teammate::communication::AgentIdentity],
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Option<PtyRuntimeIdentity> {
    identities
        .iter()
        .find(|identity| {
            identity.workspace_id == workspace_id.to_string()
                && identity.pane_id == pane_id.to_string()
                && identity.online
                && identity.generation != 0
                && !identity.lease.trim().is_empty()
        })
        .map(|identity| PtyRuntimeIdentity {
            agent_id: identity.agent_id.clone(),
            generation: identity.generation,
            lease: identity.lease.clone(),
        })
}

fn current_pty_runtime_identity(
    state: &crate::state::AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Result<Option<PtyRuntimeIdentity>, String> {
    {
        let workspaces = state.workspaces.read();
        let Some(workspace) = workspaces.get(&workspace_id) else {
            return Err(format!("workspace {workspace_id} does not exist"));
        };
        if !workspace.pane_tree.panes.contains_key(&pane_id)
            || !workspace.terminals.contains_key(&pane_id)
        {
            return Ok(None);
        }
    }
    let Some(endpoint) = crate::kernel_lifecycle::read_endpoint() else {
        return Ok(None);
    };
    let roster = ridge_kernel::client::read_domain_agent_roster(&endpoint)?;
    Ok(kernel_identity_for_pane(
        &roster.agent_identities,
        workspace_id,
        pane_id,
    ))
}

/// Desktop-only identity read used by the production sampler. Returning null
/// for an ordinary shell pane is intentional; the sampler must not invent an
/// Agent identity from a pane title or process name.
pub(crate) fn pty_runtime_identity(
    state: &crate::state::AppState,
    workspace_id: &str,
    pane_id: &str,
) -> Result<Value, String> {
    let workspace_id = Uuid::parse_str(workspace_id).map_err(|_| "workspaceId must be a UUID")?;
    let pane_id = Uuid::parse_str(pane_id).map_err(|_| "paneId must be a UUID")?;
    let identity = current_pty_runtime_identity(state, workspace_id, pane_id)?;
    Ok(identity
        .map(|identity| {
            json!({
                "agentId": identity.agent_id,
                "generation": identity.generation,
                "lease": identity.lease,
            })
        })
        .unwrap_or(Value::Null))
}

/// Accept a complete host-observed PTY sample only after checking the current
/// pane identity and live PTY. The sample is then published to the same Hub
/// registry used by delivery selection; no UI-local proof can bypass it.
pub(crate) fn publish_pty_runtime_snapshot(
    state: &crate::state::AppState,
    args: Value,
) -> Result<(), String> {
    let (workspace_id, pane_id, record) = parse_pty_runtime_request(&args)?;
    let live_and_owned = {
        let workspaces = state.workspaces.read();
        let Some(workspace) = workspaces.get(&workspace_id) else {
            return Err(format!("workspace {workspace_id} does not exist"));
        };
        workspace.pane_tree.panes.contains_key(&pane_id)
            && workspace.terminals.contains_key(&pane_id)
    };
    if !live_and_owned {
        return Err("PTY runtime snapshot identity is not a live Agent pane".into());
    }
    let Some(identity) = current_pty_runtime_identity(state, workspace_id, pane_id)? else {
        return Err("PTY runtime snapshot has no current Kernel Agent identity".into());
    };
    if identity.agent_id != record.agent_id
        || identity.generation != record.generation
        || identity.lease != record.lease
    {
        return Err("PTY runtime snapshot identity is stale".into());
    }

    let mut records = desktop_pty_runtime()
        .lock()
        .map_err(|_| "desktop PTY runtime sampler lock poisoned".to_string())?;
    let key = (workspace_id, pane_id);
    let previous = records.insert(key, record.clone());
    if let Err(error) = desktop_hub_state().register_pty_runtime_snapshot(
        record.agent_id,
        record.generation,
        record.lease,
        record.snapshot,
    ) {
        match previous {
            Some(previous) => {
                records.insert(key, previous);
            }
            None => {
                records.remove(&key);
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Remove the desktop sampler record before a pane can be reused by another
/// Agent generation. Hub teardown is fenced with the record's identity so a
/// late cleanup cannot remove a newer proof.
pub(crate) fn clear_pty_runtime_snapshot(workspace_id: Uuid, pane_id: Uuid) {
    let record = desktop_pty_runtime()
        .lock()
        .ok()
        .and_then(|mut records| records.remove(&(workspace_id, pane_id)));
    let Some(record) = record else {
        return;
    };
    let _ = desktop_hub_state().unregister_pty_runtime_snapshot(
        &record.agent_id,
        record.generation,
        &record.lease,
    );
}

pub(crate) fn desktop_hub_state() -> Arc<ridge_mcp::server::McpSessionState> {
    static STATE: OnceLock<Arc<ridge_mcp::server::McpSessionState>> = OnceLock::new();
    STATE
        .get_or_init(|| {
            Arc::new(
                ridge_mcp::server::McpSessionState::with_sqlite(
                    ridge_kernel::registry::agent_hub_path(),
                )
                .expect("open persistent Agent Hub"),
            )
        })
        .clone()
}

/// Desktop IPC entrypoint for the shared Hub. It intentionally calls the
/// public MCP tool adapter rather than writing PTY bytes; agents may consume
/// the resulting inbox through `ridge_fetch_inbox`.
pub(crate) fn send_hub_message(
    state: &crate::state::AppState,
    handle: tauri::AppHandle,
    arguments: Value,
) -> Result<Value, String> {
    let token = state
        .teammate_binding
        .read()
        .as_ref()
        .map(|binding| binding.token.clone())
        .unwrap_or_default();
    let host = DesktopMcpHost {
        ctx: TeammateCtx {
            state: state.clone(),
            token: Arc::new(token),
            handle,
        },
    };
    let response = ridge_mcp::server::call_tool_rpc(
        "ridge_send_message",
        arguments,
        &host,
        &desktop_hub_state(),
    );
    if let Some(error) = response.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Agent Hub rejected message")
            .to_string());
    }
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "Agent Hub returned an invalid response".to_string())?;
    serde_json::from_str(text).map_err(|error| format!("decode Agent Hub receipt: {error}"))
}

impl DesktopMcpHost {
    fn wid(&self) -> Uuid {
        *self.ctx.state.active_workspace.read()
    }

    /// 花名册是否含 agent：侧表 map（auto/手标）∪ typed profiles。
    fn roster_contains(&self, wid: Uuid, agent_id: &str) -> bool {
        if super::profiles::contains_agent(wid, agent_id) {
            return true;
        }
        self.ctx
            .state
            .workspaces
            .read()
            .get(&wid)
            .is_some_and(|ws| ws.teammate_agent_pane_map.contains_key(agent_id))
    }

    fn agent_id_for_pane_any(&self, wid: Uuid, pane: Uuid) -> Option<String> {
        if let Some(id) = super::profiles::agent_id_for_pane(wid, pane) {
            return Some(id);
        }
        let map = self.ctx.state.workspaces.read();
        let ws = map.get(&wid)?;
        ws.teammate_agent_pane_map
            .iter()
            .find(|(_, p)| **p == pane)
            .map(|(id, _)| id.clone())
    }

    fn workspace_id(&self, workspace_id: Option<&str>) -> HostResult<Uuid> {
        let wid = match workspace_id {
            Some(raw) => Uuid::parse_str(raw)
                .map_err(|_| HostError::InvalidParams(format!("invalid workspaceId: {raw}")))?,
            None => self.wid(),
        };
        if self.ctx.state.workspaces.read().contains_key(&wid) {
            Ok(wid)
        } else {
            Err(HostError::InvalidParams(format!("workspace {wid} 不存在")))
        }
    }

    /// 寻址始终校验复合身份；仅省略 workspaceId 时兼容活动工作区。
    fn resolve(&self, target: &Value) -> HostResult<(Uuid, Uuid)> {
        let wid = self.workspace_id(target.get("workspaceId").and_then(Value::as_str))?;
        let pane_target = target
            .get("paneId")
            .or_else(|| target.get("paneIndex"))
            .unwrap_or(target);
        match parse_pane_target(pane_target) {
            Ok(PaneTarget::Uuid(u)) => {
                if pane::teammate_pane_is_leaf(&self.ctx.state, wid, u) {
                    Ok((wid, u))
                } else {
                    Err(HostError::InvalidParams(format!(
                        "pane {u} 不在 workspace {wid}"
                    )))
                }
            }
            Ok(PaneTarget::Index(idx)) => {
                pane::teammate_pane_uuid_at_index(&self.ctx.state, wid, idx)
                    .map(|pid| (wid, pid))
                    .map_err(|e| HostError::InvalidParams(e.to_string()))
            }
            Err(e) => Err(HostError::InvalidParams(e)),
        }
    }

    fn profile_program(profile: &str) -> Option<std::path::PathBuf> {
        let path = std::env::var_os("PATH")?;
        #[cfg(windows)]
        let suffixes = [".exe", ".cmd", ".bat"];
        #[cfg(not(windows))]
        let suffixes = [""];
        for dir in std::env::split_paths(&path) {
            for suffix in suffixes {
                let candidate = dir.join(format!("{profile}{suffix}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn launch_profiles() -> Vec<LaunchProfile> {
        // 与 agent_catalog 内置 id 对齐（含 grok）。
        ["codex", "claude", "gemini", "grok"]
            .into_iter()
            .filter(|id| Self::profile_program(id).is_some())
            .map(|id| LaunchProfile {
                id: id.to_string(),
                // Desktop 不猜宿主模型/effort；空集令 core 拒绝 typed 覆盖。
                models: Vec::new(),
                reasoning_efforts: Vec::new(),
                supports_checkpoint: matches!(id, "codex" | "claude" | "grok")
                    && Self::profile_program(id)
                        .and_then(|path| path.extension().map(|ext| ext.to_owned()))
                        .and_then(|ext| ext.to_str().map(str::to_string))
                        .is_none_or(|ext| {
                            !ext.eq_ignore_ascii_case("cmd") && !ext.eq_ignore_ascii_case("bat")
                        }),
            })
            .collect()
    }

    fn canonical_target(wid: Uuid, target: &Value) -> Value {
        let raw_target = target
            .get("paneId")
            .or_else(|| target.get("paneIndex"))
            .cloned()
            .unwrap_or_else(|| target.clone());
        json!({
            "workspaceId": wid.to_string(),
            "paneId": raw_target,
        })
    }

    fn launch_command(
        profile: &str,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
        checkpoint: Option<&str>,
    ) -> HostResult<terminal::StructuredPtyCommand> {
        if model.is_some() || reasoning_effort.is_some() {
            return Err(HostError::InvalidParams(format!(
                "launch profile {profile} 未声明 model/reasoningEffort 覆盖"
            )));
        }
        let executable = Self::profile_program(profile).ok_or_else(|| {
            HostError::InvalidParams(format!("launch profile {profile} 当前不可用"))
        })?;
        let args = Self::checkpoint_args(profile, checkpoint)?;
        #[cfg(windows)]
        {
            let ext = executable
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat") {
                if checkpoint.is_some() {
                    return Err(HostError::InvalidParams(format!(
                        "launch profile {profile} 的脚本包装器不支持安全 checkpoint argv"
                    )));
                }
                return Ok(terminal::StructuredPtyCommand {
                    program: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
                    args: vec![
                        "/d".into(),
                        "/s".into(),
                        "/c".into(),
                        format!("\"{}\"", executable.display()),
                    ],
                    env: Default::default(),
                });
            }
        }
        Ok(terminal::StructuredPtyCommand {
            program: executable.to_string_lossy().into_owned(),
            args,
            env: Default::default(),
        })
    }

    fn checkpoint_args(profile: &str, checkpoint: Option<&str>) -> HostResult<Vec<String>> {
        Ok(match (profile, checkpoint) {
            ("codex", Some(value)) => vec!["resume".into(), value.into()],
            ("claude", Some(value)) => vec!["--resume".into(), value.into()],
            ("grok", Some(value)) => vec!["--resume".into(), value.into()],
            (_, Some(_)) => {
                return Err(HostError::InvalidParams(format!(
                    "launch profile {profile} 不支持 checkpoint"
                )))
            }
            (_, None) => Vec::new(),
        })
    }

    fn split_in_workspace(
        &self,
        wid: Uuid,
        direction: &str,
        role: &str,
        initial_cmd: Option<&str>,
        launch: Option<(&str, Option<&str>, Option<&str>, Option<&str>)>,
    ) -> HostResult<Value> {
        if initial_cmd.is_some() && launch.is_some() {
            return Err(HostError::InvalidParams(
                "initial_cmd 与 launch_profile 互斥".into(),
            ));
        }
        let structured = launch
            .map(|(profile, model, effort, checkpoint)| {
                Self::launch_command(profile, model, effort, checkpoint)
            })
            .transpose()?;
        let state = &self.ctx.state;
        let (idx, cwd) = {
            let map = state.workspaces.read();
            let ws = map
                .get(&wid)
                .ok_or_else(|| HostError::InvalidParams(format!("workspace {wid} 不存在")))?;
            let leaves = ws.pane_tree.get_all_leaves();
            let idx = pane::choose_balanced_split(ws)
                .and_then(|(uuid, _)| leaves.iter().position(|p| *p == uuid))
                .unwrap_or(0);
            let cwd = leaves
                .get(idx)
                .and_then(|pid| ws.pane_tree.panes.get(pid))
                .and_then(|p| p.cwd.clone());
            (idx, cwd)
        };
        let dir = if direction.eq_ignore_ascii_case("horizontal") {
            "horizontal"
        } else {
            "vertical"
        };
        let new_id = pane::teammate_split_pane(state, wid, idx, dir)
            .map_err(|e| HostError::Internal(e.to_string()))?;
        let new_idx = {
            let map = state.workspaces.read();
            map.get(&wid)
                .and_then(|ws| {
                    ws.pane_tree
                        .get_all_leaves()
                        .iter()
                        .position(|u| *u == new_id)
                })
                .unwrap_or(0)
        };
        let trace_id = Uuid::new_v4().to_string();
        if let Err(e) = terminal::ensure_pane_pty_workspace(
            state,
            wid,
            new_id,
            terminal::EnsurePtyOptions {
                shell: None,
                cwd: cwd.as_deref(),
                initial_command: initial_cmd,
                structured_command: structured,
                tmux_pane_index: Some(new_idx),
                ready_tx: None,
                trace_id: Some(trace_id.clone()),
            },
        ) {
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                let _ = ws.pane_tree.close(new_id);
                ws.pane_sizes.remove(&new_id);
            }
            return Err(HostError::Internal(format!("PTY init failed: {e}")));
        }
        {
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                ws.teammate_tmux_pane_cursor = new_idx;
                ws.pane_sizes.insert(new_id, (80, 120));
                ws.teammate_pane_titles.insert(new_id, role.to_string());
                if initial_cmd.is_some() || launch.is_some() {
                    ws.teammate_pane_states.insert(new_id, PaneState::Busy);
                }
            }
        }
        // 后台 workspace 的 split 不得把当前 UI 焦点指向异工作区 pane。
        if wid == self.wid() {
            let _ = self
                .ctx
                .handle
                .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::split(trace_id));
            let _ = self
                .ctx
                .handle
                .emit("teammate-active-pane-changed", new_id.to_string());
        }
        let (profile, model, effort, checkpoint) = launch
            .map(|(profile, model, effort, checkpoint)| (Some(profile), model, effort, checkpoint))
            .unwrap_or((None, None, None, None));
        Ok(json!({
            "workspaceId": wid.to_string(),
            "paneId": new_id.to_string(),
            "paneIndex": new_idx,
            "role": role,
            "launchProfile": profile,
            "model": model,
            "reasoningEffort": effort,
            "checkpointTransferred": checkpoint.is_some(),
            "commandSummary": profile.map(|id| format!("{id} argv launch")),
            "terminalAccepted": true,
        }))
    }
}

impl McpHost for DesktopMcpHost {
    fn team_profile(&self) -> Value {
        team_profile_snapshot(&self.ctx, self.wid())
    }

    fn probe_delivery(&self, _target: &Value) -> HostResult<DeliveryProbe> {
        let mut probe = desktop_hub_state().delivery_probe(_target);
        probe.mcp_pull = true;
        Ok(probe)
    }

    fn pty_runtime_snapshot(&self, target: &Value) -> HostResult<Option<HubPtyRuntimeSnapshot>> {
        let Some(workspace_id) = target
            .get("workspaceId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return Ok(None);
        };
        let Some(pane_id) = target
            .get("paneId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return Ok(None);
        };
        let Some(agent_id) = target.get("agentId").and_then(Value::as_str) else {
            return Ok(None);
        };
        let generation = target.get("generation").and_then(Value::as_u64);
        let lease = target.get("lease").and_then(Value::as_str);
        let workspaces = self.ctx.state.workspaces.read();
        let Some(workspace) = workspaces.get(&workspace_id) else {
            return Ok(None);
        };
        let live_and_owned = workspace.pane_tree.panes.contains_key(&pane_id)
            && workspace.terminals.contains_key(&pane_id);
        if !live_and_owned {
            return Ok(None);
        }
        drop(workspaces);

        let records = desktop_pty_runtime()
            .lock()
            .map_err(|_| HostError::Internal("desktop PTY runtime sampler lock poisoned".into()))?;
        Ok(records
            .get(&(workspace_id, pane_id))
            .filter(|record| {
                record.agent_id == agent_id
                    && Some(record.generation) == generation
                    && Some(record.lease.as_str()) == lease
            })
            .map(|record| record.snapshot))
    }

    fn deliver_runtime_api(&self, target: &Value, entry: &Value) -> HostResult<DeliveryOutcome> {
        desktop_hub_state()
            .deliver_registered_endpoint(HubDeliveryAdapter::RuntimeApi, target, entry)
            .map_err(HostError::Internal)
    }

    fn deliver_a2a(&self, target: &Value, entry: &Value) -> HostResult<DeliveryOutcome> {
        desktop_hub_state()
            .deliver_a2a_endpoint(target, entry)
            .map_err(HostError::Internal)
    }

    fn list_workspaces(&self) -> HostResult<Value> {
        let active = self.wid();
        let map = self.ctx.state.workspaces.read();
        let names = self.ctx.state.workspace_names.read();
        let order = self.ctx.state.workspace_order.read();
        let workspaces = order
            .iter()
            .filter(|id| map.contains_key(id))
            .map(|id| {
                json!({
                    "workspaceId": id.to_string(),
                    "name": names.get(id),
                    "active": *id == active,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "activeWorkspaceId": active.to_string(),
            "workspaces": workspaces,
        }))
    }

    fn team_profile_for(&self, workspace_id: Option<&str>) -> HostResult<Value> {
        let wid = self.workspace_id(workspace_id)?;
        Ok(team_profile_snapshot(&self.ctx, wid))
    }

    fn launch_capabilities(&self) -> HostResult<LaunchCapabilities> {
        Ok(LaunchCapabilities {
            profiles: Self::launch_profiles(),
        })
    }

    fn resolve_pane_target(&self, workspace_id: Option<&str>, target: &Value) -> HostResult<Value> {
        let embedded_workspace = target.get("workspaceId").and_then(Value::as_str);
        let wid = self.workspace_id(workspace_id.or(embedded_workspace))?;
        if let (Some(requested), Some(embedded)) = (workspace_id, embedded_workspace) {
            let requested = self.workspace_id(Some(requested))?;
            let embedded = self.workspace_id(Some(embedded))?;
            if embedded != requested {
                return Err(HostError::InvalidParams(
                    "workspaceId 与 target.workspaceId 不一致".into(),
                ));
            }
        }
        let scoped = Self::canonical_target(wid, target);
        let (_, pid) = self.resolve(&scoped)?;
        Ok(json!({
            "workspaceId": wid.to_string(),
            "paneId": pid.to_string(),
        }))
    }

    fn send_text(
        &self,
        target: &Value,
        text: &str,
        submit: bool,
        mark_busy: bool,
    ) -> HostResult<InputDispatch> {
        let (wid, pid) = self.resolve(target)?;
        // Enter 必须是 **CR**（0x0D）：LF 在 Claude Code/Cursor 这类 raw-mode TUI 里只是
        // 「插入换行」，消息会停在输入框永不提交（实测：整段落进对端 composer 没发出去）。
        // 与 `send-keys` 路由的既有口径一致（那里也把 \r 视作已提交）。
        let payload = if submit {
            ridge_mcp::server::enter_terminated(text)
        } else {
            text.to_string()
        };
        // G1：MCP 文本注入同属 agent 写路径，走 suspend 收口。
        super::suspend::agent_pty_write(&self.ctx.state, wid, pid, payload.as_bytes())
            .map_err(|e| HostError::Internal(e.to_string()))?;
        if mark_busy {
            let mut map = self.ctx.state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                ws.teammate_pane_states.insert(pid, PaneState::Busy);
            }
        }
        Ok(InputDispatch {
            // `agent_pty_write` reached `write_all + flush`; this proves the
            // local terminal transport accepted bytes, not agent consumption.
            terminal_accepted: true,
        })
    }

    fn report_execution_rejection(&self, report: ExternalExecutionRejection) -> HostResult<String> {
        Ok(super::hitl::report_external_rejection(
            &self.ctx.handle,
            &report,
        ))
    }

    fn capture_pane(&self, target: &Value, lines: usize) -> HostResult<String> {
        let (wid, pid) = self.resolve(target)?;
        // 用 pane 自己的尺寸重放 scrollback，取**渲染后**的屏幕文本：直接回原始字节
        // 会把 claude 这类全屏 TUI 的光标寻址/重绘一股脑塞给调用方，读不出人话。
        let (rows, cols) = {
            let map = self.ctx.state.workspaces.read();
            map.get(&wid)
                .and_then(|ws| ws.pane_sizes.get(&pid).copied())
                .unwrap_or((40, 120))
        };
        let chunk = self.ctx.state.get_pty_scrollback_tail(wid, pid, 512 * 1024);
        let mut term = ridge_term::term::terminal::Terminal::new(
            rows.max(1) as usize,
            cols.max(1) as usize,
            0,
        );
        term.feed(chunk.bytes.as_bytes());
        let screen = term.dump_visible_text();
        let start = screen.len().saturating_sub(lines);
        let tail: Vec<String> = screen[start..]
            .iter()
            .map(|l| l.trim_end().to_string())
            .collect();
        Ok(tail.join("\n"))
    }

    fn split_pane(
        &self,
        direction: &str,
        role: &str,
        initial_cmd: Option<&str>,
    ) -> HostResult<Value> {
        let wid = self.wid();
        self.split_in_workspace(wid, direction, role, initial_cmd, None)
    }

    fn split_pane_with(&self, request: &SplitPaneRequest) -> HostResult<Value> {
        let wid = self.workspace_id(request.workspace_id.as_deref())?;
        let launch = request.launch_profile.as_deref().map(|profile| {
            (
                profile,
                request.model.as_deref(),
                request.reasoning_effort.as_deref(),
                request.checkpoint.as_deref(),
            )
        });
        let replace_pid = request
            .replace_target
            .as_ref()
            .map(|target| {
                let scoped = Self::canonical_target(wid, target);
                self.resolve(&scoped).map(|(_, pid)| pid)
            })
            .transpose()?;
        let mut created = self.split_in_workspace(
            wid,
            &request.direction,
            &request.role,
            request.initial_cmd.as_deref(),
            launch,
        )?;
        if let Some(old_pid) = replace_pid {
            let state = self.ctx.state.clone();
            tauri::async_runtime::spawn(async move {
                let _ = pane::remote_close_pane(&state, wid, old_pid).await;
            });
            if let Some(object) = created.as_object_mut() {
                object.insert("replacedPaneId".into(), Value::String(old_pid.to_string()));
                object.insert("oldWorkerStopDispatched".into(), Value::Bool(true));
            }
        }
        Ok(created)
    }

    fn join_group(
        &self,
        group_name: &str,
        agent_id: Option<&str>,
        target: Option<&Value>,
    ) -> HostResult<()> {
        let group_name = group_name.trim();
        if group_name.is_empty() {
            return Err(HostError::InvalidParams("group_name 不能为空".into()));
        }
        // 花名册 SSOT 与 `ridge_get_team_profile` 一致：workspace.teammate_agent_pane_map
        // （含 auto:）+ typed profiles。旧路径只查 profiles → 自动识别成员永远 -32602。
        let (wid, agent_id): (Uuid, String) = match agent_id {
            Some(a) => {
                let wid = match target {
                    Some(value) => {
                        if value.get("paneId").is_some() || value.get("paneIndex").is_some() {
                            self.resolve(value)?.0
                        } else if let Some(ws) = value.get("workspaceId").and_then(Value::as_str) {
                            self.workspace_id(Some(ws))?
                        } else {
                            self.wid()
                        }
                    }
                    None => self.wid(),
                };
                (wid, a.to_string())
            }
            None => {
                let t = target.ok_or_else(|| {
                    HostError::InvalidParams("需提供 agent_id 或 target_pane_id".into())
                })?;
                let (wid, pid) = self.resolve(t)?;
                let id = self.agent_id_for_pane_any(wid, pid).ok_or_else(|| {
                    HostError::InvalidParams(format!("pane {pid} 未注册为 teammate（无 agent_id）"))
                })?;
                (wid, id)
            }
        };
        if !self.roster_contains(wid, &agent_id) {
            return Err(HostError::InvalidParams(format!(
                "agent_id {agent_id} 不在 workspace {wid} 花名册"
            )));
        }
        // 编组是前端 localStorage SSOT，后端不持有 → 事件桥投递，fire-and-forget。
        let _ = self.ctx.handle.emit(
            TEAMMATE_GROUP_ADD_MEMBER,
            json!({
                "workspaceId": wid.to_string(),
                "groupName": group_name,
                "agentId": agent_id,
            }),
        );
        Ok(())
    }

    fn report_progress(&self, from: &Value, status: &str, detail: &str) -> HostResult<()> {
        let resolved = if from.is_null() {
            None
        } else {
            Some(self.resolve(from)?)
        };
        let _ = self.ctx.handle.emit(
            "teammate://progress",
            json!({
                "status": status,
                "detail": detail,
                "workspaceId": resolved.map(|(wid, _)| wid.to_string()),
                "paneId": resolved.map(|(_, pid)| pid.to_string()),
                "exit_code": 0,
            }),
        );
        Ok(())
    }

    fn read_resource(&self, uri: &RidgeUri) -> HostResult<(String, String)> {
        let wid = self.wid();
        match uri {
            RidgeUri::WorkspaceActivePanes => Ok((
                "application/json".into(),
                team_profile_snapshot(&self.ctx, wid).to_string(),
            )),
            // 只读 .git/HEAD + 各 pane cwd：**不 spawn git**（见 CLAUDE.md「git 进程风暴」
            // 教训，agent 可高频调这条资源）。要 diff/状态明细请让 agent 自己在 pane 里跑。
            RidgeUri::WorkspaceGitStatus => {
                let map = self.ctx.state.workspaces.read();
                let mut repos: Vec<Value> = Vec::new();
                if let Some(ws) = map.get(&wid) {
                    let mut seen: Vec<std::path::PathBuf> = Vec::new();
                    for pid in ws.pane_tree.get_all_leaves() {
                        let Some(cwd) = ws.pane_tree.panes.get(&pid).and_then(|p| p.cwd.clone())
                        else {
                            continue;
                        };
                        let Some(root) = git_root(&cwd) else { continue };
                        if seen.contains(&root) {
                            continue;
                        }
                        seen.push(root.clone());
                        repos.push(json!({
                            "root": root.to_string_lossy(),
                            "branch": git_branch(&root),
                        }));
                    }
                }
                Ok((
                    "application/json".into(),
                    json!({ "repos": repos }).to_string(),
                ))
            }
            // 编辑器上下文 = 各 pane 的 cwd/标题/状态（Ridge 的「编辑器」就是终端工作区）。
            RidgeUri::WorkspaceEditorContext => {
                let map = self.ctx.state.workspaces.read();
                let mut panes: Vec<Value> = Vec::new();
                if let Some(ws) = map.get(&wid) {
                    for (i, pid) in ws.pane_tree.get_all_leaves().into_iter().enumerate() {
                        panes.push(json!({
                            "paneIndex": i,
                            "paneId": pid.to_string(),
                            "title": ws.teammate_pane_titles.get(&pid),
                            "cwd": ws.pane_tree.panes.get(&pid)
                                .and_then(|p| p.cwd.as_ref())
                                .map(|c| c.to_string_lossy().to_string()),
                            "busy": matches!(ws.teammate_pane_states.get(&pid), Some(PaneState::Busy)),
                        }));
                    }
                }
                Ok((
                    "application/json".into(),
                    json!({ "workspaceId": wid.to_string(), "panes": panes }).to_string(),
                ))
            }
            RidgeUri::Cache(_) => Err(HostError::Internal("cache 由协议内核处理".into())),
        }
    }

    fn pane_key(&self, target: &Value) -> HostResult<String> {
        self.resolve(target)
            .map(|(wid, pid)| format!("{wid}:{pid}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_target_preserves_raw_string_and_number() {
        let wid = Uuid::nil();
        let pane = Uuid::new_v4();
        let pane_string = pane.to_string();
        let raw_string = DesktopMcpHost::canonical_target(wid, &Value::String(pane_string.clone()));
        let raw_number = DesktopMcpHost::canonical_target(wid, &json!(2));

        assert_eq!(
            raw_string.get("paneId").and_then(Value::as_str),
            Some(pane_string.as_str())
        );
        assert_eq!(raw_number.get("paneId").and_then(Value::as_u64), Some(2));
        assert!(matches!(
            parse_pane_target(raw_string.get("paneId").unwrap()),
            Ok(PaneTarget::Uuid(id)) if id == pane
        ));
        assert!(matches!(
            parse_pane_target(raw_number.get("paneId").unwrap()),
            Ok(PaneTarget::Index(2))
        ));
    }

    #[test]
    fn checkpoint_uses_profile_owned_argv() {
        assert_eq!(
            DesktopMcpHost::checkpoint_args("codex", Some("session-1")).unwrap(),
            vec!["resume", "session-1"]
        );
        assert_eq!(
            DesktopMcpHost::checkpoint_args("claude", Some("session-2")).unwrap(),
            vec!["--resume", "session-2"]
        );
        assert!(DesktopMcpHost::checkpoint_args("gemini", Some("session-3")).is_err());
    }

    #[test]
    fn runtime_snapshot_parser_requires_all_five_conditions_and_epochs() {
        let args = json!({
            "workspaceId": Uuid::nil().to_string(),
            "paneId": Uuid::new_v4().to_string(),
            "agentId": "codex-1",
            "generation": 2,
            "lease": "lease-2",
            "agentIdle": true,
            "terminalModeAgentPrompt": true,
            "pendingApproval": false,
            "foregroundIsTargetAgent": true,
            "userInputCompeting": false,
            "stateRevision": 3,
            "inputEpoch": 4,
        });
        let (_, _, record) = parse_pty_runtime_request(&args).expect("complete sample");
        assert!(record.snapshot.safety.is_safe());
        assert!(record.snapshot.is_well_formed());

        for field in [
            "agentIdle",
            "terminalModeAgentPrompt",
            "pendingApproval",
            "foregroundIsTargetAgent",
            "userInputCompeting",
        ] {
            let mut missing = args.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(parse_pty_runtime_request(&missing).is_err(), "{field}");
        }
    }

    #[test]
    fn runtime_snapshot_parser_rejects_zero_revision_or_epoch() {
        let args = json!({
            "workspaceId": Uuid::nil().to_string(),
            "paneId": Uuid::new_v4().to_string(),
            "agentId": "codex-1",
            "generation": 1,
            "lease": "lease-1",
            "agentIdle": false,
            "terminalModeAgentPrompt": false,
            "pendingApproval": true,
            "foregroundIsTargetAgent": false,
            "userInputCompeting": true,
            "stateRevision": 0,
            "inputEpoch": 1,
        });
        assert!(parse_pty_runtime_request(&args).is_err());
        let mut zero_epoch = args;
        zero_epoch["stateRevision"] = json!(1);
        zero_epoch["inputEpoch"] = json!(0);
        assert!(parse_pty_runtime_request(&zero_epoch).is_err());
    }

    #[test]
    fn runtime_snapshot_parser_accepts_snake_case_aliases() {
        let args = json!({
            "workspace_id": Uuid::nil().to_string(),
            "pane_id": Uuid::new_v4().to_string(),
            "agent_id": "agent-1",
            "generation": 1,
            "lease": "lease-1",
            "agent_idle": true,
            "terminal_mode_agent_prompt": true,
            "pending_approval": false,
            "foreground_is_target_agent": true,
            "user_input_competing": false,
            "state_revision": 1,
            "input_epoch": 1,
        });
        let (_, _, record) = parse_pty_runtime_request(&args).expect("snake case sample");
        assert!(record.snapshot.safety.is_safe());
    }

    #[test]
    fn kernel_identity_for_pane_is_fenced_without_ui_agent_mapping() {
        let workspace_id = Uuid::new_v4();
        let pane_id = Uuid::new_v4();
        let identity =
            |workspace_id: Uuid, pane_id: Uuid, online: bool, generation: u64, lease: &str| {
                ridge_core::teammate::communication::AgentIdentity {
                    agent_id: format!("kernel:{pane_id}"),
                    session_id: "session-1".into(),
                    workspace_id: workspace_id.to_string(),
                    pane_id: pane_id.to_string(),
                    cwd: "C:/workspace".into(),
                    executable: "codex".into(),
                    argv: Vec::new(),
                    generation,
                    lease: lease.into(),
                    lifecycle: ridge_core::teammate::communication::AgentLifecycle::Online,
                    online,
                    last_seen_unix_ms: 1,
                    capabilities: vec!["messages".into()],
                }
            };
        let identities = vec![
            identity(Uuid::new_v4(), pane_id, true, 2, "wrong-workspace"),
            identity(workspace_id, Uuid::new_v4(), true, 2, "wrong-pane"),
            identity(workspace_id, pane_id, false, 2, "offline"),
            identity(workspace_id, pane_id, true, 0, "zero-generation"),
            identity(workspace_id, pane_id, true, 2, " "),
            identity(workspace_id, pane_id, true, 3, "lease-3"),
        ];

        assert_eq!(
            kernel_identity_for_pane(&identities, workspace_id, pane_id),
            Some(PtyRuntimeIdentity {
                agent_id: format!("kernel:{pane_id}"),
                generation: 3,
                lease: "lease-3".into(),
            })
        );
        assert!(kernel_identity_for_pane(&identities, workspace_id, Uuid::new_v4()).is_none());
    }
}
