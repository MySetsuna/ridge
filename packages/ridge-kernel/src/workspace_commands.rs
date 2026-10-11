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

pub fn dispatch(
    state: &AppState,
    method: &str,
    args: &Value,
    ctx: &Ctx,
) -> Option<CoreResult<Value>> {
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
            if let Ok(mut groups) = state.groups.lock() {
                for members in groups.values_mut() {
                    members.retain(|pane| !stopped.contains(pane));
                }
            }
            retired.insert(id);
        }
        *graph = next;
        drop(graph);
        state.business_events.workspace_list_changed();
        Ok(Value::Null)
    })())
}
