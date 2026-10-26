//! Stateful workspace commands over the Kernel's single persisted graph.
use crate::{registry::save_workspace_graph_at, server::AppState};
use ridge_core::workspace::file;
use ridge_core::{CoreError, CoreResult, Ctx};
use serde_json::{json, Value};
use uuid::Uuid;

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidArgs(message.into())
}
fn io(error: impl std::fmt::Display) -> CoreError {
    CoreError::Io(error.to_string())
}

/// Preserve the inherited cwd observation/write-back contract in the Runtime owner.
pub(crate) fn update_cwd(
    state: &AppState,
    workspace: Uuid,
    pane: Uuid,
    instance: &ridge_core::process_identity::ProcessInstance,
    cwd: &str,
    ctx: &Ctx,
) -> CoreResult<()> {
    if !std::path::Path::new(cwd).is_absolute() || ridge_core::dispatch::path_has_traversal(cwd) {
        return Err(invalid("observed cwd must be absolute without traversal"));
    }
    let permitted = |path: &str| {
        state.fs_scope.is_allowed(path) && ctx.capabilities().root_scope().is_allowed(path)
    };
    if !permitted(cwd) {
        return Err(CoreError::OutsideSandbox);
    }
    let retired = state
        .retired_workspaces
        .lock()
        .map_err(|_| invalid("workspace lifecycle unavailable"))?;
    if retired.contains(&workspace) {
        return Err(invalid("workspace was retired"));
    }
    let mut graph = state
        .workspaces
        .lock()
        .map_err(|_| invalid("workspace state unavailable"))?;
    let mut next = graph.clone();
    let metadata = next
        .workspace_mut(workspace)
        .ok_or_else(|| invalid("workspace not found"))?;
    ridge_core::process_identity::validate(instance).map_err(io)?;
    let current = state.ptys.info(pane).map_err(io)?;
    if current.workspace_id != Some(workspace)
        || current.child_pid != Some(instance.pid)
        || current.child_start_instance.as_deref() != Some(instance.start_instance.as_str())
    {
        return Err(invalid("PTY instance changed"));
    }
    let Some(target) = metadata.pane_tree.panes.get_mut(&pane) else {
        return state.ptys.record_cwd(pane, instance, cwd).map_err(io);
    };
    if target.cwd.as_deref() == Some(std::path::Path::new(cwd)) {
        return state.ptys.record_cwd(pane, instance, cwd).map_err(io);
    }
    target.cwd = Some(cwd.into());
    let saved = if let Some(association) = &metadata.file {
        if !permitted(&association.path) {
            return Err(CoreError::OutsideSandbox);
        }
        let path = std::path::PathBuf::from(&association.path);
        let original = std::fs::read(&path).map_err(io)?;
        let typed: file::RidgeFile = serde_json::from_slice(&original).map_err(io)?;
        file::validate(&typed).map_err(invalid)?;
        let mut document: Value = serde_json::from_slice(&original).map_err(io)?;
        // A topology autosave may still be debouncing. Merge the current graph,
        // not a single pane into an older file that cannot yet contain it.
        crate::workspace_files::merge_pane_tree(
            &mut document, &serde_json::to_value(&metadata.pane_tree).map_err(io)?
        ).map_err(invalid)?;
        document["foreign_panes"] = serde_json::to_value(&metadata.foreign_panes).map_err(io)?;
        document["saved_at"] = json!(chrono::Utc::now().to_rfc3339());
        file::validate(&serde_json::from_value(document.clone()).map_err(io)?).map_err(invalid)?;
        let archive = path.with_extension(format!("ridge.archive-{}", Uuid::new_v4()));
        file::write_atomic(&archive, &original).map_err(io)?;
        file::write_atomic(&path, &serde_json::to_vec_pretty(&document).map_err(io)?)
            .map_err(io)?;
        Some((path, original, archive))
    } else {
        None
    };
    if let Err(error) = save_workspace_graph_at(&state.workspaces_path, &next) {
        if let Some((path, original, archive)) = saved {
            file::write_atomic(&path, &original).map_err(|rollback| {
                io(format!(
                    "cwd commit failed: {error}; rollback failed: {rollback}; original at {}",
                    archive.display()
                ))
            })?;
        }
        return Err(io(error));
    }
    *graph = next;
    state.ptys.record_cwd(pane, instance, cwd).map_err(io)?;
    drop(graph);
    drop(retired);
    crate::workspace_autosave::committed(workspace);
    use ridge_core::EventSink;
    state.business_events.broadcast(
        "pane-meta-changed",
        json!({"workspaceId":workspace,"paneId":pane,"cwd":cwd}),
    );
    state.business_events.broadcast(
        &format!("pane-cwd-changed-{workspace}-{pane}"),
        json!({"cwd":cwd}),
    );
    state.business_events.pane_tree_changed(workspace);
    Ok(())
}

pub fn dispatch(
    state: &AppState,
    method: &str,
    args: &Value,
    ctx: &Ctx,
) -> Option<CoreResult<Value>> {
    if method == "dock_pane" { return Some(dock(state, args, ctx)); }
    if !matches!(
        method,
        "rename_workspace" | "reorder_workspaces" | "close_workspace"
    ) {
        return None;
    }
    Some((|| {
        let mut retired = state
            .retired_workspaces
            .lock()
            .map_err(|_| invalid("workspace lifecycle unavailable"))?;
        let mut graph = state
            .workspaces
            .lock()
            .map_err(|_| invalid("workspace state unavailable"))?;
        let mut next = graph.clone();
        let mut saved_file = None;
        let mut closed = None;
        match method {
            "rename_workspace" => {
                let id = args["workspaceId"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .ok_or_else(|| invalid("invalid workspaceId"))?;
                let name = args["name"]
                    .as_str()
                    .ok_or_else(|| invalid("missing name"))?;
                let meta = next
                    .workspace(id)
                    .ok_or_else(|| invalid("workspace not found"))?;
                if let Some(association) = &meta.file {
                    if !state.fs_scope.is_allowed(&association.path)
                        || !ctx
                            .capabilities()
                            .root_scope()
                            .is_allowed(&association.path)
                    {
                        return Err(CoreError::OutsideSandbox);
                    }
                    let path = std::path::PathBuf::from(&association.path);
                    let original = std::fs::read(&path).map_err(io)?;
                    let typed: file::RidgeFile = serde_json::from_slice(&original).map_err(io)?;
                    file::validate(&typed).map_err(invalid)?;
                    let mut document: Value = serde_json::from_slice(&original).map_err(io)?;
                    document["name"] = json!(name);
                    document["saved_at"] = json!(chrono::Utc::now().to_rfc3339());
                    let archive = path.with_extension(format!("ridge.archive-{}", Uuid::new_v4()));
                    file::write_atomic(&archive, &original).map_err(io)?;
                    file::write_atomic(&path, &serde_json::to_vec_pretty(&document).map_err(io)?)
                        .map_err(io)?;
                    saved_file = Some((path, original, archive));
                }
                next.rename(id, name.into())?;
            }
            "reorder_workspaces" => {
                let index = |key| {
                    args[key]
                        .as_u64()
                        .and_then(|value| usize::try_from(value).ok())
                        .ok_or_else(|| invalid(format!("invalid {key}")))
                };
                next.reorder(index("fromIndex")?, index("toIndex")?)?;
            }
            _ => {
                let id = args["workspaceId"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .ok_or_else(|| invalid("invalid workspaceId"))?;
                if next.len() <= 1 {
                    return Err(invalid("cannot close the last workspace"));
                }
                next.remove_workspace(id)
                    .ok_or_else(|| invalid("workspace not found"))?;
                let original = std::fs::read(&state.workspaces_path).map_err(io)?;
                let archive = state
                    .workspaces_path
                    .with_extension(format!("json.archive-{}", Uuid::new_v4()));
                file::write_atomic(&archive, &original).map_err(io)?;
                closed = Some((id, original, archive));
            }
        }
        let persisted = if let Some((_, original, _)) = &closed {
            // Check replacement permissions before killing children, while
            // retaining the same metadata on disk until teardown completes.
            file::write_atomic(&state.workspaces_path, original).map_err(anyhow::Error::from)
        } else {
            save_workspace_graph_at(&state.workspaces_path, &next)
        };
        if let Err(error) = persisted {
            if let Some((path, original, archive)) = saved_file {
                if let Err(rollback) = file::write_atomic(&path, &original) {
                    return Err(io(format!("workspace commit failed: {error}; file rollback failed: {rollback}; original retained at {}",archive.display())));
                }
            }
            return Err(io(error));
        }
        if let Some((id, _original, archive)) = closed {
            let mut stopped = Vec::new();
            let result = (|| -> Result<(), String> {
                for pty in state
                    .ptys
                    .list()
                    .into_iter()
                    .filter(|pty| pty.workspace_id == Some(id))
                {
                    state
                        .ptys
                        .destroy(pty.id)
                        .map_err(|error| error.to_string())?;
                    stopped.push(pty.id);
                    crate::domain::remove_agent_identity_for_pty(state, pty.id)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                return Err(io(format!("workspace close incomplete: {error}; stopped PTYs: {stopped:?}; metadata retained; original retained at {}",archive.display())));
            }
            if let Err(error) = save_workspace_graph_at(&state.workspaces_path, &next) {
                return Err(io(format!("workspace close incomplete after stopping PTYs {stopped:?}: {error}; metadata retained; original retained at {}",archive.display())));
            }
            // Group identities and removed workspaces stay persisted for recovery.
            retired.insert(id);
        }
        *graph = next;
        drop(graph);
        state.business_events.workspace_list_changed();
        Ok(Value::Null)
    })())
}

fn dock(state: &AppState, args: &Value, ctx: &Ctx) -> CoreResult<Value> {
    use ridge_core::workspace::pane_tree::DockRegion;
    let pane = |key| args[key].as_str().and_then(|value| Uuid::parse_str(value).ok()).ok_or_else(|| invalid(format!("invalid {key}")));
    let source = pane("sourcePaneId")?;
    let target = pane("targetPaneId")?;
    let region = match args["region"].as_str().unwrap_or_default().to_ascii_lowercase().as_str() {
        "left" => DockRegion::Left, "right" => DockRegion::Right, "top" => DockRegion::Top,
        "bottom" => DockRegion::Bottom, "center" => DockRegion::Center, _ => return Err(invalid("invalid dock region")),
    };
    let permitted = |path: &str| !ridge_core::dispatch::path_has_traversal(path)
        && state.fs_scope.is_allowed(path) && ctx.capabilities().root_scope().is_allowed(path);
    let mut retired = state.retired_workspaces.lock().map_err(|_| invalid("workspace lifecycle unavailable"))?;
    let mut graph = state.workspaces.lock().map_err(|_| invalid("workspace unavailable"))?;
    let locate = |pane| -> CoreResult<Uuid> {
        let ids = graph.workspace_ids().flat_map(|id| graph.workspace(*id).unwrap().pane_tree.get_all_leaves().into_iter().filter(move |leaf| *leaf == pane).map(move |_| *id)).collect::<Vec<_>>();
        match ids.as_slice() { [id] if !retired.contains(id) => Ok(*id), _ => Err(invalid("pane identity missing or ambiguous")) }
    };
    let from = locate(source)?;
    let to = locate(target)?;
    let mut saves = Vec::new();
    for (workspace, pane) in [(from, source), (to, target)] {
        let metadata = graph.workspace(workspace).unwrap();
        let leaf = metadata.pane_tree.panes.get(&pane).ok_or(CoreError::PaneNotFound(pane))?;
        if leaf.cwd.as_ref().is_some_and(|path| !permitted(&path.to_string_lossy())) { return Err(CoreError::OutsideSandbox); }
        if let ridge_core::workspace::mode::PaneMode::Editor { file_path: Some(path), .. } = &leaf.mode {
            if !permitted(&path.to_string_lossy()) { return Err(CoreError::OutsideSandbox); }
        }
        if let Some(file) = &metadata.file {
            if !permitted(&file.path) { return Err(CoreError::OutsideSandbox); }
            if !saves.iter().any(|request: &crate::workspace_files::Request| request.workspace_id == Some(workspace)) {
                saves.push(crate::workspace_files::Request { operation: "auto-save".into(), workspace_id: Some(workspace), name: metadata.name.clone(), path: Some(file.path.clone()) });
            }
        }
    }
    if from != to {
        let roster = state.roster.lock().map_err(|_| invalid("Agent roster unavailable"))?;
        if roster.agent_identities().iter().any(|identity| identity.pane_id == source.to_string() && identity.protocol_binding.is_some() && !roster.protocol_member_retired(&identity.agent_id)) {
            return Err(invalid("Agent protocol is bound to its registered workspace; remove membership before moving this pane across workspaces"));
        }
    }
    let mut next = graph.clone();
    let closed = next.dock_between(from, to, source, target, region)?;
    if closed {
        let original = std::fs::read(&state.workspaces_path).map_err(io)?;
        file::write_atomic(&state.workspaces_path.with_extension(format!("json.archive-{}", Uuid::new_v4())), &original).map_err(io)?;
        saves.retain(|request| request.workspace_id != Some(from));
    }
    let commit = || -> CoreResult<()> {
        save_workspace_graph_at(&state.workspaces_path, &next).map_err(io)?;
        *graph = next;
        if closed { retired.insert(from); }
        Ok(())
    };
    if from != to { state.ptys.retarget_workspace(source, from, to, commit)?; } else { commit()?; }
    drop(graph); drop(retired);
    state.business_events.pane_tree_changed(from);
    if from != to { state.business_events.pane_tree_changed(to); }
    if closed { state.business_events.workspace_list_changed(); }
    for request in saves {
        crate::workspace_autosave::schedule(state.clone(), request).map_err(|error| io(format!("Dock committed; autosave could not be scheduled: {error}")))?;
    }
    Ok(Value::Null)
}
