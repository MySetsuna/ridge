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
    ExternalExecutionRejection, HostError, HostResult, InputDispatch, McpHost,
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

    /// 寻址：`paneId`(Uuid 串) 或 `paneIndex`(数字)，都校验落在**当前活动工作区**。
    fn resolve(&self, target: &Value) -> HostResult<Uuid> {
        let wid = self.wid();
        match parse_pane_target(target) {
            Ok(PaneTarget::Uuid(u)) => {
                if pane::teammate_pane_is_leaf(&self.ctx.state, wid, u) {
                    Ok(u)
                } else {
                    Err(HostError::InvalidParams(format!(
                        "pane {u} 不在当前活动工作区"
                    )))
                }
            }
            Ok(PaneTarget::Index(idx)) => {
                pane::teammate_pane_uuid_at_index(&self.ctx.state, wid, idx)
                    .map_err(|e| HostError::InvalidParams(e.to_string()))
            }
            Err(e) => Err(HostError::InvalidParams(e)),
        }
    }
}

impl McpHost for DesktopMcpHost {
    fn team_profile(&self) -> Value {
        team_profile_snapshot(&self.ctx, self.wid())
    }

    fn send_text(
        &self,
        target: &Value,
        text: &str,
        submit: bool,
        mark_busy: bool,
    ) -> HostResult<InputDispatch> {
        let wid = self.wid();
        let pid = self.resolve(target)?;
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

    fn report_execution_rejection(
        &self,
        report: ExternalExecutionRejection,
    ) -> HostResult<String> {
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
        let wid = self.wid();
        let pid = self.resolve(target)?;
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
        let state = &self.ctx.state;
        // 目标叶子：与 GUI/`split-window` 同一口径（面积最大叶子），方向按调用方要求。
        let (idx, cwd) = {
            let map = state.workspaces.read();
            let ws = map
                .get(&wid)
                .ok_or_else(|| HostError::Internal("no active workspace".into()))?;
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
            None,
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
                // role 落成 pane 标题：花名册里看得见「这是谁」，跨 agent 协作靠它认人。
                ws.teammate_pane_titles.insert(new_id, role.to_string());
                if initial_cmd.is_some() {
                    ws.teammate_pane_states.insert(new_id, PaneState::Busy);
                }
            }
        }
        let _ = self
            .ctx
            .handle
            .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::split(trace_id));
        let _ = self
            .ctx
            .handle
            .emit("teammate-active-pane-changed", new_id.to_string());
        Ok(json!({ "paneId": new_id.to_string(), "paneIndex": new_idx, "role": role }))
    }

    fn join_group(
        &self,
        group_name: &str,
        agent_id: Option<&str>,
        target: Option<&Value>,
    ) -> HostResult<()> {
        let wid = self.wid();
        let agent_id: String = match agent_id {
            Some(a) => a.to_string(),
            None => {
                let t = target.ok_or_else(|| {
                    HostError::InvalidParams("需提供 agent_id 或 target_pane_id".into())
                })?;
                let pid = self.resolve(t)?;
                super::profiles::agent_id_for_pane(wid, pid).ok_or_else(|| {
                    HostError::InvalidParams(format!("pane {pid} 未注册为 teammate（无 agent_id）"))
                })?
            }
        };
        if !super::profiles::contains_agent(wid, &agent_id) {
            return Err(HostError::InvalidParams(format!(
                "agent_id {agent_id} 不在当前活动工作区花名册"
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
        let pid = if from.is_null() {
            None
        } else {
            Some(self.resolve(from)?)
        };
        let _ = self.ctx.handle.emit(
            "teammate://progress",
            json!({
                "status": status,
                "detail": detail,
                "paneId": pid.map(|p| p.to_string()),
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
                Ok(("application/json".into(), json!({ "repos": repos }).to_string()))
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
        self.resolve(target).map(|u| u.to_string())
    }
}
