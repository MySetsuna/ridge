//! Kernel-owned portable workspace saves/imports. Import never spawns a PTY
//! or Agent; live processes retain their existing registry identities.
use crate::{registry::save_workspace_graph_at, server::AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use ridge_core::workspace::{
    file::{self, FileAssociation, RidgeFile, RIDGE_FILE_VERSION},
    graph::WorkspaceMeta,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub operation: String,
    pub workspace_id: Option<Uuid>,
    pub name: Option<String>,
    pub path: Option<String>,
}

fn default_dir() -> PathBuf {
    std::env::var_os("RIDGE_WORKSPACE_SAVE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("ridge-workspaces")))
        .unwrap_or_else(|| PathBuf::from("ridge-workspaces"))
}

fn filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || "-_. ".contains(ch) {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let value = cleaned.trim().trim_matches('.');
    if value.is_empty() {
        "workspace.ridge".into()
    } else {
        format!("{value}.ridge")
    }
}

fn target(path: Option<&str>, name: &str) -> Result<PathBuf, String> {
    let value = path
        .map(PathBuf::from)
        .unwrap_or_else(|| default_dir().join(filename(name)));
    let absolute = if value.is_absolute() {
        value
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(value)
    };
    if !absolute
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("ridge"))
    {
        return Err("workspace file must use .ridge extension".into());
    }
    Ok(absolute)
}

fn save(state: &AppState, request: &Request) -> Result<Value, String> {
    let id = request.workspace_id.ok_or("missing workspaceId")?;
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or("workspace name must not be empty")?;
    let path = target(request.path.as_deref(), name)?;
    let mut graph = state
        .workspaces
        .lock()
        .map_err(|_| "workspace state unavailable")?;
    let mut next = graph.clone();
    let meta = next.workspace_mut(id).ok_or("workspace unavailable")?;
    let association = meta.file.clone().unwrap_or_default();
    let payload = RidgeFile {
        version: RIDGE_FILE_VERSION,
        name: name.into(),
        saved_at: chrono::Utc::now().to_rfc3339(),
        pane_tree: serde_json::to_value(&meta.pane_tree).map_err(|error| error.to_string())?,
        git_repos: association.git_repos.clone(),
        index_path: association.index_path.clone(),
        pane_titles: association.pane_titles.clone(),
    };
    file::validate(&payload)?;
    // Keep the prior bytes, including unknown legacy fields, before replacing
    // an existing document. A failed state commit remains recoverable.
    let backup = if path.exists() {
        let backup = path.with_extension(format!("ridge.archive-{}", Uuid::new_v4()));
        std::fs::copy(&path, &backup).map_err(|error| error.to_string())?;
        Some(backup)
    } else {
        None
    };
    file::write(&path, &payload)?;
    meta.name = Some(name.into());
    meta.file = Some(FileAssociation {
        path: path.to_string_lossy().into(),
        ..association
    });
    if let Err(error) = save_workspace_graph_at(&state.workspaces_path, &next) {
        if let Some(backup) = &backup {
            let restored =
                std::fs::read(backup).and_then(|bytes| file::write_atomic(&path, &bytes));
            if let Err(restore) = restored {
                return Err(format!("association commit failed: {error}; snapshot rollback failed: {restore}; prior bytes retained in {}",backup.display()));
            }
            return Err(format!(
                "association commit failed; previous snapshot restored: {error}"
            ));
        }
        return Err(format!(
            "snapshot exported to {}; association commit failed: {error}",
            path.display()
        ));
    }
    *graph = next;
    drop(graph);
    state.business_events.workspace_list_changed();
    Ok(json!({"path":path,"backupPath":backup,"workspaceId":id}))
}

fn open(state: &AppState, path: &str) -> Result<Value, String> {
    let source = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let payload = file::read(&source)?;
    let tree = file::validate(&payload)?;
    let source = source.to_string_lossy().to_string();
    let mut graph = state
        .workspaces
        .lock()
        .map_err(|_| "workspace state unavailable")?;
    if let Some(id) = graph
        .workspace_ids()
        .find(|id| {
            graph
                .workspace(**id)
                .and_then(|meta| meta.file.as_ref())
                .is_some_and(|file| {
                    std::fs::canonicalize(&file.path)
                        .ok()
                        .is_some_and(|path| path.to_string_lossy() == source)
                })
        })
        .copied()
    {
        return Ok(json!({"workspaceId":id,"alreadyOpen":true}));
    }
    // Exact pane identity must not be guessed or silently remapped into a
    // different live workspace. Conflicting legacy copies remain untouched.
    if graph.workspace_ids().any(|id| {
        graph.workspace(*id).is_some_and(|meta| {
            tree.panes
                .keys()
                .any(|pane| meta.pane_tree.panes.contains_key(pane))
        })
    }) {
        return Err("workspace pane identities already belong to another workspace; source retained unchanged".into());
    }
    let id = Uuid::new_v4();
    let mut next = graph.clone();
    next.insert_workspace(
        id,
        WorkspaceMeta {
            pane_tree: tree,
            name: Some(payload.name),
            locked_sizes: HashMap::new(),
            file: Some(FileAssociation {
                path: source,
                git_repos: payload.git_repos,
                index_path: payload.index_path,
                pane_titles: payload.pane_titles,
            }),
        },
    );
    save_workspace_graph_at(&state.workspaces_path, &next).map_err(|error| error.to_string())?;
    *graph = next;
    drop(graph);
    state.business_events.workspace_list_changed();
    Ok(json!({"workspaceId":id,"alreadyOpen":false}))
}

fn handle(state: &AppState, request: Request) -> Result<Value, String> {
    match request.operation.as_str() {
        "save" => save(state, &request),
        "open" => open(state, request.path.as_deref().ok_or("missing path")?),
        "default-directory" => Ok(json!(default_dir())),
        "info" => {
            let graph = state
                .workspaces
                .lock()
                .map_err(|_| "workspace state unavailable")?;
            let id = request.workspace_id.ok_or("missing workspaceId")?;
            let meta = graph.workspace(id).ok_or("workspace unavailable")?;
            Ok(
                json!({"workspace_id":id,"file_path":meta.file.as_ref().map(|file|&file.path),"name":meta.name}),
            )
        }
        "associations" => {
            let graph = state
                .workspaces
                .lock()
                .map_err(|_| "workspace state unavailable")?;
            Ok(json!(graph.workspace_ids().filter_map(|id|graph.workspace(*id).map(|meta|json!({"workspace_id":id,"file_path":meta.file.as_ref().map(|file|&file.path),"name":meta.name}))).collect::<Vec<_>>()))
        }
        "list" => {
            let directory = default_dir();
            if !directory.exists() {
                return Ok(json!([]));
            }
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
                let path = entry.map_err(|error| error.to_string())?.path();
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("ridge"))
                {
                    match file::read(&path) {
                        Ok(payload) => entries.push(
                            json!({"name":payload.name,"path":path,"mtime_secs":std::fs::metadata(&path).and_then(|m|m.modified()).ok().and_then(|time|time.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_secs()).unwrap_or(0)}),
                        ),
                        Err(error) => entries.push(json!({"path":path,"error":error})),
                    }
                }
            }
            entries
                .sort_by_key(|entry| std::cmp::Reverse(entry["mtime_secs"].as_u64().unwrap_or(0)));
            Ok(json!(entries))
        }
        _ => Err("unsupported workspace file operation".into()),
    }
}

pub async fn endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Result<Json<Value>, StatusCode> {
    if headers
        .get("x-ridge-kernel-token")
        .and_then(|v| v.to_str().ok())
        != Some(state.token.as_str())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let result = tokio::task::spawn_blocking(move || handle(&state, request))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(match result {
        Ok(value) => json!({"ok":true,"source":"ridge-kernel","value":value}),
        Err(error) => json!({"ok":false,"source":"ridge-kernel","error":error}),
    }))
}
