//! Adapter from the authoritative Kernel state to the shared Ridge MCP engine.

use std::sync::Arc;

use ridge_mcp::delivery::{DeliveryOutcome, DeliveryProbe, HubDeliveryAdapter};
use ridge_mcp::resource::{git_branch, git_root, RidgeUri};
use ridge_mcp::server::{
    HostError, HostResult, InputDispatch, LaunchCapabilities, LaunchProfile, McpHost,
    SplitPaneRequest,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent_profiles::{builtin_profiles, AgentProfile};
use crate::pty::{PtyBridge, PtyLaunch};
use crate::registry::{save_roster_at, save_workspace_graph_at};
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
                let id = Uuid::parse_str(raw).map_err(|_| {
                    HostError::InvalidParams(format!("invalid workspace id: {raw}"))
                })?;
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
                    Uuid::parse_str(id)
                        .map_err(|_| HostError::InvalidParams(format!("invalid pane id: {id}")))?
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
            Value::Number(number) => {
                self.pane_at(number.as_u64().ok_or_else(|| {
                    HostError::InvalidParams("pane index must be non-negative".into())
                })? as usize)?
            }
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
        let identities = {
            let roster = self.state.roster.lock().expect("roster lock");
            roster
                .agent_identities()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };
        let roster = if identities.is_empty() {
            self.state
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
                .collect::<Vec<_>>()
        } else {
            identities
                .clone()
                .into_iter()
                .filter(|identity| {
                    workspace_id.is_none()
                        || identity.workspace_id == workspace_id.unwrap().to_string()
                })
                .map(|identity| {
                    json!({
                        "id": identity.agent_id,
                        "agentId": identity.agent_id,
                        "sessionId": identity.session_id,
                        "paneId": identity.pane_id,
                        "workspaceId": identity.workspace_id,
                        "cwd": identity.cwd,
                        "executable": identity.executable,
                        "argv": identity.argv,
                        "generation": identity.generation,
                        "lease": identity.lease,
                        "lifecycle": identity.lifecycle,
                        "online": identity.online,
                        "lastSeenUnixMs": identity.last_seen_unix_ms,
                        "capabilities": identity.capabilities,
                    })
                })
                .collect::<Vec<_>>()
        };
        let groups = self
            .state
            .groups
            .lock()
            .expect("agent groups lock")
            .iter()
            .map(|(name, members)| {
                json!({
                    "name": name,
                    "members": members.iter().map(|id| {
                        identities
                            .iter()
                            .find(|identity| identity.pane_id == id.to_string())
                            .map(|identity| identity.agent_id.clone())
                            .unwrap_or_else(|| format!("kernel:{id}"))
                    }).collect::<Vec<_>>()
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

    fn split_workspace(
        graph: &mut ridge_core::workspace::graph::WorkspaceGraph,
        request: &SplitPaneRequest,
        direction: ridge_core::workspace::pane_tree::SplitDirection,
    ) -> HostResult<(Uuid, Uuid)> {
        match request.workspace_id.as_deref() {
            Some(raw) => {
                let workspace_id = Uuid::parse_str(raw).map_err(|_| {
                    HostError::InvalidParams(format!("invalid workspace id: {raw}"))
                })?;
                let anchor = graph
                    .leaves(workspace_id)
                    .map_err(|error| HostError::InvalidParams(error.to_string()))?
                    .into_iter()
                    .last()
                    .ok_or_else(|| HostError::Internal("workspace has no pane".into()))?;
                let pane_id = graph
                    .split(workspace_id, anchor, direction)
                    .map_err(|error| HostError::Internal(error.to_string()))?;
                Ok((workspace_id, pane_id))
            }
            None => match graph.active() {
                Some(workspace_id) => {
                    let anchor = graph
                        .leaves(workspace_id)
                        .map_err(|error| HostError::Internal(error.to_string()))?
                        .into_iter()
                        .last()
                        .ok_or_else(|| HostError::Internal("workspace has no pane".into()))?;
                    let pane_id = graph
                        .split(workspace_id, anchor, direction)
                        .map_err(|error| HostError::Internal(error.to_string()))?;
                    Ok((workspace_id, pane_id))
                }
                None => {
                    let workspace_id = graph.create_workspace();
                    let pane_id = graph
                        .leaves(workspace_id)
                        .map_err(|error| HostError::Internal(error.to_string()))?
                        .into_iter()
                        .next()
                        .ok_or_else(|| HostError::Internal("new workspace has no pane".into()))?;
                    Ok((workspace_id, pane_id))
                }
            },
        }
    }

    fn split_direction(raw: &str) -> HostResult<ridge_core::workspace::pane_tree::SplitDirection> {
        match raw {
            "horizontal" => Ok(ridge_core::workspace::pane_tree::SplitDirection::Horizontal),
            "vertical" => Ok(ridge_core::workspace::pane_tree::SplitDirection::Vertical),
            _ => Err(HostError::InvalidParams(
                "direction must be horizontal or vertical".into(),
            )),
        }
    }

    fn prepare_replacement(
        &self,
        graph: &mut ridge_core::workspace::graph::WorkspaceGraph,
        workspace_id: Uuid,
        request: &SplitPaneRequest,
    ) -> HostResult<(Option<Uuid>, Option<Arc<PtyBridge>>)> {
        let replace_id = request
            .replace_target
            .as_ref()
            .map(|target| self.pane_id(target))
            .transpose()?;
        let Some(old_id) = replace_id else {
            return Ok((None, None));
        };
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
        graph
            .close(workspace_id, old_id)
            .map_err(|error| HostError::InvalidParams(error.to_string()))?;
        let bridge = self
            .state
            .ptys
            .begin_destroy(old_id)
            .map_err(|error| HostError::InvalidParams(error.to_string()))?;
        Ok((Some(old_id), Some(bridge)))
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_split_pty(
        &self,
        request: &SplitPaneRequest,
        pane_id: Uuid,
        workspace_id: Uuid,
        program: Option<&str>,
        args: &[String],
        launch_profile: Option<&str>,
        replace_id: Option<Uuid>,
    ) -> HostResult<()> {
        self.state
            .ptys
            .spawn_command_for(PtyLaunch {
                id: pane_id,
                program,
                args,
                cwd: None,
                workspace_id: Some(workspace_id),
                role: &request.role,
                launch_profile,
                env: None,
            })
            .map_err(|error| {
                if let Some(old_id) = replace_id {
                    self.state.ptys.cancel_destroy(old_id);
                }
                HostError::Internal(error.to_string())
            })
            .map(|_| ())
    }

    fn write_initial_command(
        &self,
        request: &SplitPaneRequest,
        pane_id: Uuid,
        replace_id: Option<Uuid>,
    ) -> HostResult<()> {
        let Some(initial_cmd) = request.initial_cmd.as_deref() else {
            return Ok(());
        };
        self.state
            .ptys
            .write(
                pane_id,
                ridge_mcp::server::enter_terminated(initial_cmd).as_bytes(),
            )
            .map_err(|error| {
                let _ = self.state.ptys.destroy(pane_id);
                if let Some(old_id) = replace_id {
                    self.state.ptys.cancel_destroy(old_id);
                }
                HostError::Internal(error.to_string())
            })
    }

    fn commit_split_identity(
        &self,
        request: &SplitPaneRequest,
        pane_id: Uuid,
        program: Option<&str>,
        args: &[String],
        replace_id: Option<Uuid>,
        previous: &ridge_core::workspace::graph::WorkspaceGraph,
    ) -> HostResult<Option<ridge_core::teammate::topology::TopologyGraph>> {
        if request.role.trim().eq_ignore_ascii_case("shell") {
            return Ok(None);
        }
        let previous_roster = self.state.roster.lock().expect("roster lock").clone();
        let executable = program.unwrap_or("agent");
        if let Err(error) = crate::domain::commit_agent_identity_for_pty(
            &self.state,
            pane_id,
            &request.role,
            executable,
            args.to_vec(),
        ) {
            let _ = self.state.ptys.destroy(pane_id);
            if let Some(old_id) = replace_id {
                self.state.ptys.cancel_destroy(old_id);
            }
            let _ = save_workspace_graph_at(&self.state.workspaces_path, previous);
            return Err(HostError::Internal(format!(
                "Agent identity commit failed; graph rolled back: {error}"
            )));
        }
        Ok(Some(previous_roster))
    }

    fn restore_roster(&self, roster: Option<&ridge_core::teammate::topology::TopologyGraph>) {
        if let Some(roster) = roster {
            let _ = save_roster_at(&self.state.roster_path, roster);
            *self.state.roster.lock().expect("roster lock") = roster.clone();
        }
    }

    fn finalize_replacement(
        &self,
        replace_id: Option<Uuid>,
        old_bridge: Option<Arc<PtyBridge>>,
        pane_id: Uuid,
        previous: &ridge_core::workspace::graph::WorkspaceGraph,
        previous_roster: Option<&ridge_core::teammate::topology::TopologyGraph>,
    ) -> HostResult<()> {
        let (Some(old_id), Some(bridge)) = (replace_id, old_bridge) else {
            return Ok(());
        };
        if let Err(error) = bridge.destroy() {
            self.state.ptys.cancel_destroy(old_id);
            let _ = save_workspace_graph_at(&self.state.workspaces_path, previous);
            let _ = self.state.ptys.destroy(pane_id);
            self.restore_roster(previous_roster);
            return Err(HostError::Internal(format!(
                "replacement PTY could not be destroyed; graph rolled back: {error}"
            )));
        }
        if let Err(error) = self.state.ptys.finish_destroy(old_id) {
            let _ = save_workspace_graph_at(&self.state.workspaces_path, previous);
            let _ = self.state.ptys.destroy(pane_id);
            self.restore_roster(previous_roster);
            return Err(HostError::Internal(format!(
                "replacement PTY registry commit failed; graph rolled back: {error}"
            )));
        }
        crate::domain::remove_agent_identity_for_pty(&self.state, old_id).map_err(|error| {
            HostError::Internal(format!(
                "replacement PTY destroyed but old Agent identity teardown failed: {error}"
            ))
        })?;
        self.state
            .mcp_state
            .purge_pane(&old_id.to_string())
            .map_err(HostError::Internal)
    }

    fn split_impl(&self, request: &SplitPaneRequest) -> HostResult<Value> {
        let (program, args, launch_profile) = Self::launch_plan(request)?;
        let direction = Self::split_direction(&request.direction)?;

        let mut graph = self.state.workspaces.lock().expect("workspace graph lock");
        let previous = graph.clone();
        let mut next = previous.clone();
        let (workspace_id, pane_id) = Self::split_workspace(&mut next, request, direction)?;

        let (replace_id, old_bridge) =
            Self::prepare_replacement(self, &mut next, workspace_id, request)?;

        self.spawn_split_pty(
            request,
            pane_id,
            workspace_id,
            program.as_deref(),
            &args,
            launch_profile.as_deref(),
            replace_id,
        )?;
        self.write_initial_command(request, pane_id, replace_id)?;
        if let Err(error) = save_workspace_graph_at(&self.state.workspaces_path, &next) {
            let _ = self.state.ptys.destroy(pane_id);
            if let Some(old_id) = replace_id {
                self.state.ptys.cancel_destroy(old_id);
            }
            return Err(HostError::Internal(error.to_string()));
        }

        let previous_roster = self.commit_split_identity(
            request,
            pane_id,
            program.as_deref(),
            &args,
            replace_id,
            &previous,
        )?;
        self.finalize_replacement(
            replace_id,
            old_bridge,
            pane_id,
            &previous,
            previous_roster.as_ref(),
        )?;
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

    fn probe_delivery(&self, _target: &Value) -> HostResult<DeliveryProbe> {
        let mut probe = self.state.mcp_state.delivery_probe(_target);
        // MCP pull remains available for every fenced target. Runtime/A2A is
        // advertised only when a current host-owned route is registered.
        probe.mcp_pull = true;
        Ok(probe)
    }

    fn deliver_runtime_api(&self, target: &Value, entry: &Value) -> HostResult<DeliveryOutcome> {
        self.state
            .mcp_state
            .deliver_registered_endpoint(HubDeliveryAdapter::RuntimeApi, target, entry)
            .map_err(HostError::Internal)
    }

    fn deliver_a2a(&self, target: &Value, entry: &Value) -> HostResult<DeliveryOutcome> {
        self.state
            .mcp_state
            .deliver_registered_endpoint(HubDeliveryAdapter::A2a, target, entry)
            .map_err(HostError::Internal)
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
                })).collect::<Vec<_>>() })
                .to_string(),
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
                Ok((
                    "application/json".into(),
                    json!({ "repos": roots }).to_string(),
                ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::oneshot;

    fn test_state() -> AppState {
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        AppState {
            token: "test-token".into(),
            pid: 1,
            port: 0,
            started_at_unix: 0,
            shutdown_tx: Arc::new(std::sync::Mutex::new(Some(shutdown_tx))),
            shutting_down: Arc::new(AtomicBool::new(false)),
            workspaces: Arc::new(std::sync::Mutex::new(
                ridge_core::workspace::graph::WorkspaceGraph::new(),
            )),
            workspaces_path: std::env::temp_dir().join("ridge-kernel-kernel-mcp-workspaces.json"),
            roster: Arc::new(std::sync::Mutex::new(
                ridge_core::teammate::topology::TopologyGraph::new(),
            )),
            roster_path: std::env::temp_dir().join("ridge-kernel-kernel-mcp-roster.json"),
            groups: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            mcp_state: Arc::new(ridge_mcp::server::McpSessionState::default()),
            remote_hosts: Arc::new(std::sync::Mutex::new(
                ridge_core::remote::RemoteHostTopology::default(),
            )),
            remote_hosts_path: std::env::temp_dir().join("ridge-kernel-kernel-mcp-remote.json"),
            ptys: Arc::new(crate::pty::PtyRegistry::default()),
            output_leases: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            fs_scope: ridge_core::sandbox::RootScope::unrestricted(),
        }
    }

    #[test]
    fn production_kernel_host_dispatches_registered_runtime_route() {
        let state = test_state();
        let receiver = state
            .mcp_state
            .register_delivery_endpoint(HubDeliveryAdapter::RuntimeApi, "agent-a", 2, "lease-2")
            .unwrap();
        let host = KernelMcpHost::new(state);
        let target = json!({
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        });
        assert!(host.probe_delivery(&target).unwrap().runtime_api);
        let entry = json!({"messageId":"kernel-runtime-1"});
        let outcome = host.deliver_runtime_api(&target, &entry).unwrap();
        assert!(outcome.accepted);
        assert_eq!(receiver.recv().unwrap(), entry);
    }
}
