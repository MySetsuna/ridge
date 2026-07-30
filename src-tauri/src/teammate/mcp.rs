//! 桌面宿主的 MCP 实装：把 `ridge_mcp::server::McpHost` 落到 `AppState`。
//!
//! 协议分发、工具语义、WS/HTTP 传输都在 `ridge-mcp` crate 里，与 rdg 无头 host
//! 共用同一份；本文件只回答「怎么动桌面工作区的 pane」。

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::Emitter;
use uuid::Uuid;

use ridge_mcp::addressing::{parse_pane_target, PaneTarget};
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
    mcp_router(McpTransportCtx::new(
        Arc::new(DesktopMcpHost { ctx }),
        token,
        env!("CARGO_PKG_VERSION"),
    ))
}

// ─── 宿主实装 ────────────────────────────────────────────────────────────────

struct DesktopMcpHost {
    ctx: TeammateCtx,
}

impl DesktopMcpHost {
    fn wid(&self) -> Uuid {
        *self.ctx.state.active_workspace.read()
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
        ["codex", "claude", "gemini"]
            .into_iter()
            .filter(|id| Self::profile_program(id).is_some())
            .map(|id| LaunchProfile {
                id: id.to_string(),
                // Desktop 不猜宿主模型/effort；空集令 core 拒绝 typed 覆盖。
                models: Vec::new(),
                reasoning_efforts: Vec::new(),
                supports_checkpoint: matches!(id, "codex" | "claude")
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
            None,
            cwd.as_deref(),
            initial_cmd,
            structured,
            Some(new_idx),
            None,
            Some(trace_id.clone()),
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
            &report.initiator,
            &report.action,
            &report.executor,
            &report.policy_source,
            &report.request_id,
            &report.reason,
            &report.next_step,
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
        let (wid, agent_id): (Uuid, String) = match agent_id {
            Some(a) => {
                let wid = match target {
                    Some(value) => self.resolve(value)?.0,
                    None => self.wid(),
                };
                (wid, a.to_string())
            }
            None => {
                let t = target.ok_or_else(|| {
                    HostError::InvalidParams("需提供 agent_id 或 target_pane_id".into())
                })?;
                let (wid, pid) = self.resolve(t)?;
                let id = super::profiles::agent_id_for_pane(wid, pid).ok_or_else(|| {
                    HostError::InvalidParams(format!("pane {pid} 未注册为 teammate（无 agent_id）"))
                })?;
                (wid, id)
            }
        };
        if !super::profiles::contains_agent(wid, &agent_id) {
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
}
