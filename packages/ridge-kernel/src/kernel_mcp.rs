//! Adapter from the authoritative Kernel state to the shared Ridge MCP engine.

use std::sync::Arc;

use ridge_mcp::resource::{git_branch, git_root, RidgeUri};
use ridge_mcp::server::{
    HostError, HostResult, InputDispatch, LaunchCapabilities, LaunchProfile, McpHost,
    SplitPaneRequest,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent_profiles::{builtin_profiles, AgentProfile};
use crate::registry::save_workspace_graph_at;
use crate::server::AppState;

#[derive(Clone)]
pub struct KernelMcpHost {
    state: AppState,
}

impl KernelMcpHost {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn router(state: AppState, token: Arc<String>) -> axum::Router {
        let mcp_state = state.mcp_state.clone();
        ridge_mcp::transport::mcp_router(ridge_mcp::transport::McpTransportCtx::with_state(
            Arc::new(Self::new(state)),
            token,
            env!("CARGO_PKG_VERSION"),
            mcp_state,
        ))
    }

    fn workspace_id(&self, requested: Option<&str>) -> HostResult<Uuid> {
        let graph = self.state.workspaces.lock().expect("workspace graph lock");
        match requested {
            Some(raw) => {
                let id = Uuid::parse_str(raw)
                    .map_err(|_| HostError::InvalidParams(format!("invalid workspace id: {raw}")))?;
                if graph.workspace_ids().any(|candidate| *candidate == id) {
                    Ok(id)
                } else {
                    Err(HostError::InvalidParams(format!(
                        "workspace does not exist: {id}"
                    )))
                }
            }
            None => graph
                .active()
                .ok_or_else(|| HostError::InvalidParams("kernel has no active workspace".into())),
        }
    }

    fn pane_id(&self, target: &Value) -> HostResult<Uuid> {
        let candidate = match target {
            Value::Object(object) => {
                if let Some(id) = object.get("paneId").and_then(Value::as_str) {
                    Uuid::parse_str(id).map_err(|_| {
                        HostError::InvalidParams(format!("invalid pane id: {id}"))
                    })?
                } else if let Some(index) = object.get("paneIndex").and_then(Value::as_u64) {
                    self.pane_at(index as usize)?
                } else {
                    return Err(HostError::InvalidParams(
                        "target requires paneId or paneIndex".into(),
                    ));
                }
            }
            Value::String(raw) => match Uuid::parse_str(raw) {
                Ok(id) => id,
                Err(_) => match raw.parse::<usize>() {
                    Ok(index) => self.pane_at(index)?,
                    Err(_) => {
                        return Err(HostError::InvalidParams(format!(
                            "invalid pane target: {raw}"
                        )))
                    }
                },
            },
            Value::Number(number) => self.pane_at(
                number
                    .as_u64()
                    .ok_or_else(|| HostError::InvalidParams("pane index must be non-negative".into()))?
                    as usize,
            )?,
            _ => {
                return Err(HostError::InvalidParams(
                    "pane target must be UUID, index, or object".into(),
                ))
            }
        };
        self.state
            .ptys
            .info(candidate)
            .map_err(|_| HostError::InvalidParams(format!("pane does not exist: {candidate}")))?;
        Ok(candidate)
    }

    fn pane_at(&self, index: usize) -> HostResult<Uuid> {
        self.state
            .ptys
            .list()
            .into_iter()
            .find(|info| info.pane_index == index)
            .map(|info| info.id)
            .ok_or_else(|| HostError::InvalidParams(format!("pane index out of range: {index}")))
    }

    fn team_profile_snapshot(&self, workspace_id: Option<Uuid>) -> Value {
        let roster = self
            .state
            .ptys
            .list()
            .into_iter()
            .filter(|info| workspace_id.is_none() || info.workspace_id == workspace_id)
            .map(|info| {
                json!({
                    "id": format!("kernel:{}", info.id),
                    "name": info.launch_profile.as_deref().unwrap_or(&info.role),
                    "paneId": info.id,
                    "paneIndex": info.pane_index,
                    "workspaceId": info.workspace_id,
                    "role": info.role,
                    "status": info.status,
                    "cwd": info.cwd,
                    "launchProfile": info.launch_profile,
                })
            })
            .collect::<Vec<_>>();
        let groups = self
            .state
            .groups
            .lock()
            .expect("agent groups lock")
            .iter()
            .map(|(name, members)| {
                json!({
                    "name": name,
                    "members": members.iter().map(|id| format!("kernel:{id}")).collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        json!({ "roster": roster, "leaderId": null, "edges": [], "groups": groups })
    }

    fn launch_profile(id: &str) -> HostResult<AgentProfile> {
        builtin_profiles()
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| HostError::InvalidParams(format!("unknown launch profile: {id}")))
    }

    fn launch_plan(
        request: &SplitPaneRequest,
    ) -> HostResult<(Option<String>, Vec<String>, Option<String>)> {
        let Some(profile_id) = request.launch_profile.as_deref() else {
            return Ok((None, Vec::new(), None));
        };
        let profile = Self::launch_profile(profile_id)?;
        let args = match request.checkpoint.as_deref() {
            Some(checkpoint) => {
                if profile.resume_argv.is_empty() {
                    return Err(HostError::InvalidParams(format!(
                        "launch profile {profile_id} does not support checkpoint"
                    )));
                }
                profile
                    .resume_argv
                    .iter()
                    .map(|arg| arg.replace("{session}", checkpoint))
                    .collect()
            }
            None => Vec::new(),
        };
        Ok((Some(profile.executable), args, Some(profile.id)))
    }

    fn split_impl(&self, request: &SplitPaneRequest) -> HostResult<Value> {
        let (program, args, launch_profile) = Self::launch_plan(request)?;
        let direction = match request.direction.as_str() {
            "horizontal" => ridge_core::workspace::pane_tree::SplitDirection::Horizontal,
            "vertical" => ridge_core::workspace::pane_tree::SplitDirection::Vertical,
            _ => {
                return Err(HostError::InvalidParams(
                    "direction must be horizontal or vertical".into(),
                ))
            }
        };

        let mut graph = self.state.workspaces.lock().expect("workspace graph lock");
        let previous = graph.clone();
        let mut next = previous.clone();
        let (workspace_id, pane_id) = match request.workspace_id.as_deref() {
            Some(raw) => {
                let workspace_id = Uuid::parse_str(raw).map_err(|_| {
                    HostError::InvalidParams(format!("invalid workspace id: {raw}"))
                })?;
                let anchor = next
                    .leaves(workspace_id)
                    .map_err(|error| HostError::InvalidParams(error.to_string()))?
                    .into_iter()
                    .last()
                    .ok_or_else(|| HostError::Internal("workspace has no pane".into()))?;
                let pane_id = next
                    .split(workspace_id, anchor, direction)
                    .map_err(|error| HostError::Internal(error.to_string()))?;
                (workspace_id, pane_id)
            }
            None => match next.active() {
                Some(workspace_id) => {
                    let anchor = next
                        .leaves(workspace_id)
                        .map_err(|error| HostError::Internal(error.to_string()))?
                        .into_iter()
                        .last()
                        .ok_or_else(|| HostError::Internal("workspace has no pane".into()))?;
                    let pane_id = next
                        .split(workspace_id, anchor, direction)
                        .map_err(|error| HostError::Internal(error.to_string()))?;
                    (workspace_id, pane_id)
                }
                None => {
                    let workspace_id = next.create_workspace();
                    let pane_id = next
                        .leaves(workspace_id)
                        .map_err(|error| HostError::Internal(error.to_string()))?
                        .into_iter()
                        .next()
                        .ok_or_else(|| HostError::Internal("new workspace has no pane".into()))?;
                    (workspace_id, pane_id)
                }
            },
        };

        let replace_id = request
            .replace_target
            .as_ref()
            .map(|target| self.pane_id(target))
            .transpose()?;
        let mut old_bridge = None;
        if let Some(old_id) = replace_id {
            let old = self
                .state
                .ptys
                .info(old_id)
                .map_err(|error| HostError::InvalidParams(error.to_string()))?;
            if old.workspace_id != Some(workspace_id) {
                return Err(HostError::InvalidParams(
                    "replacement pane belongs to a different workspace".into(),
                ));
            }
            next.close(workspace_id, old_id)
                .map_err(|error| HostError::InvalidParams(error.to_string()))?;
            old_bridge = Some(
                self.state
                    .ptys
                    .begin_destroy(old_id)
                    .map_err(|error| HostError::InvalidParams(error.to_string()))?,
            );
        }

        self.state
            .ptys
            .spawn_command_for(
                pane_id,
                program.as_deref(),
                &args,
                None,
                Some(workspace_id),
                &request.role,
                launch_profile.as_deref(),
            )
            .map_err(|error| {
                if let Some(old_id) = replace_id {
                    self.state.ptys.cancel_destroy(old_id);
                }
                HostError::Internal(error.to_string())
            })?;
        if let Some(initial_cmd) = request.initial_cmd.as_deref() {
            if let Err(error) = self.state.ptys.write(
                pane_id,
                ridge_mcp::server::enter_terminated(initial_cmd).as_bytes(),
            ) {
                let _ = self.state.ptys.destroy(pane_id);
                if let Some(old_id) = replace_id {
                    self.state.ptys.cancel_destroy(old_id);
                }
                return Err(HostError::Internal(error.to_string()));
            }
        }
        if let Err(error) = save_workspace_graph_at(&self.state.workspaces_path, &next) {
            let _ = self.state.ptys.destroy(pane_id);
            if let Some(old_id) = replace_id {
                self.state.ptys.cancel_destroy(old_id);
            }
            return Err(HostError::Internal(error.to_string()));
        }

        if let (Some(old_id), Some(bridge)) = (replace_id, old_bridge) {
            if let Err(error) = bridge.destroy() {
                self.state.ptys.cancel_destroy(old_id);
                let _ = save_workspace_graph_at(&self.state.workspaces_path, &previous);
                let _ = self.state.ptys.destroy(pane_id);
                return Err(HostError::Internal(format!(
                    "replacement PTY could not be destroyed; graph rolled back: {error}"
                )));
            }
            if let Err(error) = self.state.ptys.finish_destroy(old_id) {
                let _ = save_workspace_graph_at(&self.state.workspaces_path, &previous);
                let _ = self.state.ptys.destroy(pane_id);
                return Err(HostError::Internal(format!(
                    "replacement PTY registry commit failed; graph rolled back: {error}"
                )));
            }
            self.state.mcp_state.purge_pane(&old_id.to_string());
        }
        *graph = next;
        drop(graph);
        Ok(json!({
            "workspaceId": workspace_id,
            "paneId": pane_id,
            "paneIndex": self.state.ptys.info(pane_id).map_err(|e| HostError::Internal(e.to_string()))?.pane_index,
            "role": request.role,
            "launchProfile": launch_profile,
            "checkpointTransferred": request.checkpoint.is_some(),
            "replacementRequested": replace_id.is_some(),
            "status": "pane_created",
            "terminalAccepted": request.initial_cmd.is_some(),
        }))
    }
}

impl McpHost for KernelMcpHost {
    fn team_profile(&self) -> Value {
        self.team_profile_snapshot(None)
    }

    fn list_workspaces(&self) -> HostResult<Value> {
        let graph = self.state.workspaces.lock().expect("workspace graph lock");
        let active = graph.active();
        let workspaces = graph
            .workspace_ids()
            .map(|id| json!({ "workspaceId": id, "active": Some(*id) == active }))
            .collect::<Vec<_>>();
        Ok(json!({ "activeWorkspaceId": active, "workspaces": workspaces }))
    }

    fn team_profile_for(&self, workspace_id: Option<&str>) -> HostResult<Value> {
        let workspace_id = self.workspace_id(workspace_id)?;
        Ok(self.team_profile_snapshot(Some(workspace_id)))
    }

    fn launch_capabilities(&self) -> HostResult<LaunchCapabilities> {
        Ok(LaunchCapabilities {
            profiles: builtin_profiles()
                .into_iter()
                .map(|profile| LaunchProfile {
                    id: profile.id,
                    models: Vec::new(),
                    reasoning_efforts: Vec::new(),
                    supports_checkpoint: !profile.resume_argv.is_empty(),
                })
                .collect(),
        })
    }

    fn resolve_pane_target(&self, workspace_id: Option<&str>, target: &Value) -> HostResult<Value> {
        let pane_id = self.pane_id(target)?;
        let info = self
            .state
            .ptys
            .info(pane_id)
            .map_err(|error| HostError::InvalidParams(error.to_string()))?;
        if let Some(workspace_id) = workspace_id {
            let requested = self.workspace_id(Some(workspace_id))?;
            if info.workspace_id != Some(requested) {
                return Err(HostError::InvalidParams(
                    "pane belongs to a different workspace".into(),
                ));
            }
        }
        Ok(json!({ "workspaceId": info.workspace_id, "paneId": pane_id }))
    }

    fn send_text(
        &self,
        target: &Value,
        text: &str,
        submit: bool,
        mark_busy: bool,
    ) -> HostResult<InputDispatch> {
        let pane_id = self.pane_id(target)?;
        let payload = if submit {
            ridge_mcp::server::enter_terminated(text)
        } else {
            text.to_string()
        };
        self.state
            .ptys
            .write(pane_id, payload.as_bytes())
            .map_err(|error| HostError::Internal(error.to_string()))?;
        if mark_busy {
            self.state
                .ptys
                .set_status(pane_id, "Working")
                .map_err(|error| HostError::Internal(error.to_string()))?;
        }
        Ok(InputDispatch {
            terminal_accepted: true,
        })
    }

    fn capture_pane(&self, target: &Value, lines: usize) -> HostResult<String> {
        let pane_id = self.pane_id(target)?;
        self.state
            .ptys
            .rendered(pane_id, lines)
            .map_err(|error| HostError::Internal(error.to_string()))
    }

    fn split_pane(
        &self,
        direction: &str,
        role: &str,
        initial_cmd: Option<&str>,
    ) -> HostResult<Value> {
        self.split_impl(&SplitPaneRequest {
            workspace_id: None,
            direction: direction.to_string(),
            role: role.to_string(),
            initial_cmd: initial_cmd.map(str::to_string),
            launch_profile: None,
            model: None,
            reasoning_effort: None,
            checkpoint: None,
            replace_target: None,
        })
    }

    fn split_pane_with(&self, request: &SplitPaneRequest) -> HostResult<Value> {
        self.split_impl(request)
    }

    fn join_group(
        &self,
        group_name: &str,
        agent_id: Option<&str>,
        target: Option<&Value>,
    ) -> HostResult<()> {
        let pane_id = match (agent_id, target) {
            (_, Some(target)) => self.pane_id(target)?,
            (Some(agent_id), None) => {
                let raw = agent_id.strip_prefix("kernel:").ok_or_else(|| {
                    HostError::InvalidParams("kernel agent id must start with kernel:".into())
                })?;
                self.pane_id(&Value::String(raw.to_string()))?
            }
            (None, None) => {
                return Err(HostError::InvalidParams(
                    "join_group requires agent_id or target_pane_id".into(),
                ))
            }
        };
        self.state
            .groups
            .lock()
            .expect("agent groups lock")
            .entry(group_name.to_string())
            .or_default()
            .insert(pane_id);
        Ok(())
    }

    fn report_progress(&self, from: &Value, status: &str, _detail: &str) -> HostResult<()> {
        if !from.is_null() {
            let pane_id = self.pane_id(from)?;
            self.state
                .ptys
                .set_status(pane_id, status)
                .map_err(|error| HostError::Internal(error.to_string()))?;
        }
        Ok(())
    }

    fn read_resource(&self, uri: &RidgeUri) -> HostResult<(String, String)> {
        match uri {
            RidgeUri::WorkspaceActivePanes => Ok((
                "application/json".into(),
                self.team_profile_snapshot(None).to_string(),
            )),
            RidgeUri::WorkspaceEditorContext => Ok((
                "application/json".into(),
                json!({ "panes": self.state.ptys.list().into_iter().map(|info| json!({
                    "workspaceId": info.workspace_id,
                    "paneId": info.id,
                    "paneIndex": info.pane_index,
                    "cwd": info.cwd,
                    "status": info.status,
                })).collect::<Vec<_>>() }).to_string(),
            )),
            RidgeUri::WorkspaceGitStatus => {
                let mut roots = Vec::new();
                for info in self.state.ptys.list() {
                    let Some(cwd) = info.cwd else { continue };
                    let Some(root) = git_root(std::path::Path::new(&cwd)) else {
                        continue;
                    };
                    if roots.iter().any(|item: &Value| {
                        item["root"].as_str() == Some(root.to_string_lossy().as_ref())
                    }) {
                        continue;
                    }
                    roots.push(json!({
                        "root": root.to_string_lossy(),
                        "branch": git_branch(&root),
                    }));
                }
                Ok(("application/json".into(), json!({ "repos": roots }).to_string()))
            }
            RidgeUri::Cache(_) => Err(HostError::Internal(
                "cache resources are handled by the shared MCP engine".into(),
            )),
        }
    }

    fn pane_key(&self, target: &Value) -> HostResult<String> {
        Ok(self.pane_id(target)?.to_string())
    }
}
