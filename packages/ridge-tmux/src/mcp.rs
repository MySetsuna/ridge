//! 无头（rdg）宿主的 MCP 实装。
//!
//! 桌面与 rdg 对同一套 MCP 能力平权：协议分发、工具语义、WS/HTTP 传输都在
//! `ridge_mcp`，这里只负责「怎么动本引擎的 pane」。无头 host 没有前端花名册/
//! 编组，身份即「会话:窗口.面板 + %id」。

use std::sync::Arc;

use serde_json::{json, Value};

use ridge_mcp::resource::{git_branch, git_root, RidgeUri};
use ridge_mcp::server::{HostError, HostResult, McpHost};
use ridge_mcp::transport::{mcp_router, McpTransportCtx};

use crate::http::{GuiSessionSource, NativeHttpCtx};
use crate::NativeError;

/// 把共享的 MCP 路由（`/api/v1/mcp/ws` + `POST /api/v1/mcp`）接到本引擎上。
pub fn router(ctx: NativeHttpCtx) -> axum::Router {
    mcp_router(McpTransportCtx::new(
        Arc::new(TmuxMcpHost::new(&ctx)),
        ctx.token.clone(),
        env!("CARGO_PKG_VERSION"),
    ))
}

// ─── 宿主实装 ────────────────────────────────────────────────────────────────

struct TmuxMcpHost {
    socket: String,
    gui: Arc<dyn GuiSessionSource>,
}

impl TmuxMcpHost {
    fn new(ctx: &NativeHttpCtx) -> Self {
        Self {
            socket: "default".to_string(),
            gui: ctx.gui.clone(),
        }
    }

    /// 归一寻址：`%N`（全局 pane id）原样；数字 → 花名册拍平序号换 `%N`。
    fn target_of(&self, target: &Value) -> HostResult<String> {
        let panes = crate::roster_panes(&self.socket);
        match target {
            Value::String(s) if s.starts_with('%') => {
                if panes.iter().any(|p| &p.pane_id == s) {
                    Ok(s.clone())
                } else {
                    Err(HostError::InvalidParams(format!("pane {s} 不存在")))
                }
            }
            Value::String(s) => match s.trim().parse::<usize>() {
                Ok(i) => index_to_id(&panes, i),
                Err(_) => Err(HostError::InvalidParams(format!("无法解析 pane 目标: {s}"))),
            },
            Value::Number(n) => match n.as_u64() {
                Some(i) => index_to_id(&panes, i as usize),
                None => Err(HostError::InvalidParams("pane 索引须为非负整数".into())),
            },
            _ => Err(HostError::InvalidParams(
                "target_pane_id 须为 %id 串或数字索引".into(),
            )),
        }
    }
}

fn index_to_id(panes: &[crate::RosterPane], idx: usize) -> HostResult<String> {
    panes
        .get(idx)
        .map(|p| p.pane_id.clone())
        .ok_or_else(|| HostError::InvalidParams(format!("pane 索引 {idx} 越界")))
}

fn engine_err(e: NativeError) -> HostError {
    match e {
        NativeError::NotFound(m) | NativeError::Ambiguous(m) | NativeError::NoServer(m) => {
            HostError::InvalidParams(m)
        }
        NativeError::Gui(name) => HostError::InvalidParams(format!("目标属 GUI 会话: {name}")),
        NativeError::Duplicate(m) | NativeError::Internal(m) => HostError::Internal(m),
    }
}

impl McpHost for TmuxMcpHost {
    fn team_profile(&self) -> Value {
        let roster: Vec<Value> = crate::roster_panes(&self.socket)
            .into_iter()
            .map(|p| {
                json!({
                    "id": p.pane_id,
                    "name": format!("{}:{}.{}", p.session, p.window, p.pane),
                    "paneId": p.pane_id,
                    "paneIndex": p.flat_index,
                    "role": "Worker",
                    "status": if p.attached { "Working" } else { "Idle" },
                    "title": p.title,
                    "cwd": p.cwd,
                })
            })
            .collect();
        json!({ "roster": roster, "leaderId": null, "edges": [], "groups": [] })
    }

    fn send_text(&self, target: &Value, text: &str, _mark_busy: bool) -> HostResult<()> {
        let t = self.target_of(target)?;
        // 与桌面一致：补 CR 作 Enter（LF 在 raw-mode TUI 里只插换行、不提交）。
        crate::send_keys(
            &self.socket,
            &t,
            &self.gui.sessions_for(&self.socket),
            &ridge_mcp::server::enter_terminated(text),
        )
            .map(|_| ())
            .map_err(engine_err)
    }

    fn capture_pane(&self, target: &Value, lines: usize) -> HostResult<String> {
        let t = self.target_of(target)?;
        crate::capture(
            &self.socket,
            &t,
            &self.gui.sessions_for(&self.socket),
            Some(lines),
        )
        .map_err(engine_err)
    }

    fn split_pane(
        &self,
        _direction: &str,
        role: &str,
        initial_cmd: Option<&str>,
    ) -> HostResult<Value> {
        // 无头会话没有几何方向（引擎里 pane 是列表），方向参数按既有 HTTP 语义忽略。
        let gui = self.gui.sessions_for(&self.socket);
        // 目标必须落到具体 pane：空 target 会被 resolve 当成「GUI 当前会话」而失败
        // （无头 host 根本没有 GUI）。取花名册第一条即本 socket 的既有会话。
        let anchor = crate::roster_panes(&self.socket)
            .first()
            .map(|p| p.pane_id.clone())
            .ok_or_else(|| {
                HostError::InvalidParams("本 host 还没有会话，先 new-session 再分屏".into())
            })?;
        crate::add_pane(
            &self.socket,
            &anchor,
            &gui,
            false,
            Some(role),
            None,
            None,
            initial_cmd,
            Some(Some("#{pane_id}")),
        )
        .map_err(engine_err)
        .map(|out| {
            let pane_id = out.trim().to_string();
            let idx = crate::roster_panes(&self.socket)
                .into_iter()
                .find(|p| p.pane_id == pane_id)
                .map(|p| p.flat_index);
            json!({ "paneId": pane_id, "paneIndex": idx, "role": role })
        })
    }

    fn read_resource(&self, uri: &RidgeUri) -> HostResult<(String, String)> {
        match uri {
            RidgeUri::WorkspaceActivePanes => {
                Ok(("application/json".into(), self.team_profile().to_string()))
            }
            // 只读 .git/HEAD，不 spawn git（agent 可能高频调用）。
            RidgeUri::WorkspaceGitStatus => {
                let mut repos: Vec<Value> = Vec::new();
                let mut seen: Vec<std::path::PathBuf> = Vec::new();
                for p in crate::roster_panes(&self.socket) {
                    let Some(cwd) = p.cwd else { continue };
                    let Some(root) = git_root(std::path::Path::new(&cwd)) else {
                        continue;
                    };
                    if seen.contains(&root) {
                        continue;
                    }
                    seen.push(root.clone());
                    repos.push(json!({
                        "root": root.to_string_lossy(),
                        "branch": git_branch(&root),
                    }));
                }
                Ok(("application/json".into(), json!({ "repos": repos }).to_string()))
            }
            RidgeUri::WorkspaceEditorContext => {
                let panes: Vec<Value> = crate::roster_panes(&self.socket)
                    .into_iter()
                    .map(|p| {
                        json!({
                            "paneIndex": p.flat_index,
                            "paneId": p.pane_id,
                            "title": p.title,
                            "cwd": p.cwd,
                            "busy": p.attached,
                        })
                    })
                    .collect();
                Ok(("application/json".into(), json!({ "panes": panes }).to_string()))
            }
            RidgeUri::Cache(_) => Err(HostError::Internal("cache 由协议内核处理".into())),
        }
    }

    fn pane_key(&self, target: &Value) -> HostResult<String> {
        self.target_of(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_to_id_reports_out_of_range() {
        let e = index_to_id(&[], 3).unwrap_err();
        assert!(matches!(e, HostError::InvalidParams(_)));
    }
}
