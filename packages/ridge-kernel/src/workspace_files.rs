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
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Deserialize)]
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

/// Overlay the current topology while retaining unknown fields on surviving panes.
pub(crate) fn merge_pane_tree(document: &mut Value, current: &Value) -> Result<(), String> {
    let mut tree = document["pane_tree"].clone();
    if !tree.is_object() { tree = json!({}); }
    tree["root"] = current["root"].clone();
    let mut panes = json!({});
    for (id, pane) in current["panes"].as_object().ok_or("invalid pane tree")? {
        let mut merged = tree["panes"][id].clone();
        if !merged.is_object() { merged = json!({}); }
        for (key, value) in pane.as_object().ok_or("invalid pane")? {
            merged[key] = value.clone();
        }
        for key in ["cwd", "shell_kind"] {
            merged[key] = pane.get(key).cloned().unwrap_or(Value::Null);
        }
        panes[id] = merged;
    }
    tree["panes"] = panes;
    document["pane_tree"] = tree;
    Ok(())
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
    let value = match path {
        None => default_dir().join(filename(name)),
        Some(path) => {
            let value = PathBuf::from(path);
            if path.to_ascii_lowercase().ends_with(".ridge") {
                value
            } else {
                value.join(filename(name))
            }
        }
    };
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

fn save(state: &AppState, request: &Request, explicit: bool) -> Result<Value, String> {
    let id = request.workspace_id.ok_or("missing workspaceId")?;
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or("workspace name must not be empty")?;
    let requested_name = name;
    let mut graph = state
        .workspaces
        .lock()
        .map_err(|_| "workspace state unavailable")?;
    let mut next = graph.clone();
    let (path, name) = if !explicit {
        let metadata = next.workspace(id).ok_or("workspace unavailable")?;
        let Some(file) = &metadata.file else {
            return Ok(Value::Null);
        };
        (
            PathBuf::from(&file.path),
            metadata
                .name
                .clone()
                .unwrap_or_else(|| requested_name.into()),
        )
    } else {
        (
            target(request.path.as_deref(), requested_name)?,
            requested_name.to_string(),
        )
    };
    if !state.fs_scope.is_allowed(&path.to_string_lossy()) {
        return Err("workspace file outside authorized roots".into());
    }
    if next.workspace_ids().any(|other| {
        *other != id
            && next
                .workspace(*other)
                .and_then(|meta| meta.file.as_ref())
                .is_some_and(|association| {
                    std::fs::canonicalize(&association.path)
                        .ok()
                        .is_some_and(|existing| {
                            std::fs::canonicalize(&path).ok().as_ref() == Some(&existing)
                        })
                })
    }) {
        return Err("workspace file is already associated with another workspace".into());
    }
    let meta = next.workspace_mut(id).ok_or("workspace unavailable")?;
    let previous = if path.exists() {
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
        let payload: RidgeFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid existing workspace; original preserved: {error}"))?;
        file::validate(&payload)?;
        Some((bytes, payload))
    } else {
        None
    };
    let mut association = meta.file.clone().unwrap_or_else(|| {
        previous
            .as_ref()
            .map(|(_, payload)| FileAssociation {
                path: path.to_string_lossy().into(),
                git_repos: payload.git_repos.clone(),
                index_path: payload.index_path.clone(),
                pane_titles: payload.pane_titles.clone(),
            })
            .unwrap_or_default()
    });
    // Fresh saves discover repository roots from the same authoritative pane cwd projection.
    for cwd in meta
        .pane_tree
        .panes
        .values()
        .filter_map(|pane| pane.cwd.as_ref())
    {
        if let Some(root) = cwd.ancestors().find(|path| path.join(".git").exists()) {
            let root = root.to_string_lossy().into_owned();
            if !association.git_repos.contains(&root) {
                association.git_repos.push(root);
            }
        }
    }
    association.git_repos.sort();
    let payload = RidgeFile {
        foreign_panes:meta.foreign_panes.clone(),
        version: RIDGE_FILE_VERSION,
        name: name.clone(),
        saved_at: chrono::Utc::now().to_rfc3339(),
        pane_tree: serde_json::to_value(&meta.pane_tree).map_err(|error| error.to_string())?,
        git_repos: association.git_repos.clone(),
        index_path: association.index_path.clone(),
        pane_titles: association.pane_titles.clone(),
    };
    file::validate(&payload)?;
    // Keep the prior bytes, including unknown legacy fields, before replacing
    // an existing document. A failed state commit remains recoverable.
    let backup = if let Some((bytes, _)) = &previous {
        let backup = path.with_extension(format!("ridge.archive-{}", Uuid::new_v4()));
        file::write_atomic(&backup, bytes).map_err(|error| error.to_string())?;
        Some(backup)
    } else {
        None
    };
    let mut document = previous
        .as_ref()
        .map(|(bytes, _)| serde_json::from_slice::<Value>(bytes))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| json!({}));
    let encoded = serde_json::to_value(&payload).map_err(|error| error.to_string())?;
    // Preserve unknown document and pane fields while replacing current authoritative values.
    merge_pane_tree(&mut document, &encoded["pane_tree"])?;
    for (key, value) in encoded.as_object().ok_or("invalid workspace")? {
        if key != "pane_tree" { document[key] = value.clone(); }
    }
    document["foreign_panes"]=serde_json::to_value(&payload.foreign_panes).map_err(|error|error.to_string())?;
    // Optional fields are omitted by serde, but must not retain stale known values.
    document["index_path"] = json!(payload.index_path);
    document["pane_titles"] = json!(payload.pane_titles);
    file::write_atomic(
        &path,
        &serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
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
    crate::workspace_autosave::committed(id);
    drop(graph);
    state.business_events.workspace_list_changed();
    if explicit {
        crate::workspace_history::record(&path.to_string_lossy(), true).map_err(|error| {
            format!("workspace saved and associated, but history update failed: {error}")
        })?;
    }
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
    let source_path = Path::new(&source);
    let session_directory = crate::workspace_history::directory().join("session-workspaces");
    if source_path.parent().is_some_and(|parent| {
        std::fs::canonicalize(&session_directory).ok().as_deref() == Some(parent)
    }) {
        if let Some(id) = source_path
            .file_stem()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("session-"))
            .and_then(|id| Uuid::parse_str(id).ok())
        {
            if let Some(metadata) = graph.workspace(id) {
                let existing = metadata
                    .pane_tree
                    .panes
                    .keys()
                    .copied()
                    .collect::<std::collections::HashSet<_>>();
                let imported = tree
                    .panes
                    .keys()
                    .copied()
                    .collect::<std::collections::HashSet<_>>();
                if existing == imported {
                    crate::workspace_history::record(&source, true).map_err(|error| {
                        format!("workspace restored; history update failed: {error}")
                    })?;
                    return Ok(json!({"workspaceId":id,"alreadyOpen":true}));
                }
                return Err("private restore snapshot conflicts with the current workspace; original preserved".into());
            }
        }
    }
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
        crate::workspace_history::record(&source, true)
            .map_err(|error| format!("workspace already open; history update failed: {error}"))?;
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
            foreign_panes:payload.foreign_panes,
            display_seq: 0,
            pane_tree: tree,
            name: Some(payload.name),
            locked_sizes: HashMap::new(),
            file: Some(FileAssociation {
                path: source.clone(),
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
    crate::workspace_history::record(&source, true)
        .map_err(|error| format!("workspace imported; history update failed: {error}"))?;
    Ok(json!({"workspaceId":id,"alreadyOpen":false}))
}

fn detach_file(state: &AppState, request: &Request, saved_only: bool) -> Result<Value, String> {
    let mut graph = state
        .workspaces
        .lock()
        .map_err(|_| "workspace state unavailable")?;
    let mut next = graph.clone();
    let path = if saved_only {
        let requested = std::fs::canonicalize(request.path.as_deref().ok_or("missing path")?)
            .map_err(|error| error.to_string())?;
        let directory = std::fs::canonicalize(default_dir()).map_err(|error| error.to_string())?;
        if requested.parent() != Some(directory.as_path())
            || !requested
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ridge"))
        {
            return Err(
                "saved workspace deletion requires a direct .ridge child of the default directory"
                    .into(),
            );
        }
        requested
    } else {
        let metadata = next
            .workspace_mut(request.workspace_id.ok_or("missing workspaceId")?)
            .ok_or("workspace unavailable")?;
        let Some(file) = &metadata.file else {
            return Ok(Value::Null);
        };
        PathBuf::from(&file.path)
    };
    if !state.fs_scope.is_allowed(&path.to_string_lossy()) {
        return Err("workspace file outside authorized roots".into());
    }
    let canonical = std::fs::canonicalize(&path).ok();
    let ids = next.workspace_ids().copied().collect::<Vec<_>>();
    for id in ids {
        let metadata = next.workspace_mut(id).unwrap();
        if metadata.file.as_ref().is_some_and(|file| {
            Path::new(&file.path) == path
                || canonical.as_ref().is_some_and(|path| {
                    std::fs::canonicalize(&file.path).ok().as_ref() == Some(path)
                })
        }) {
            metadata.file = None;
            metadata.name = None;
        }
    }
    let archive = path.with_extension(format!("ridge.archive-{}", Uuid::new_v4()));
    let existed = path.exists();
    if existed {
        std::fs::rename(&path, &archive).map_err(|error| error.to_string())?;
    }
    if let Err(error) = save_workspace_graph_at(&state.workspaces_path, &next) {
        if existed {
            std::fs::rename(&archive, &path).map_err(|rollback| {
                format!(
                    "detach failed: {error}; rollback failed: {rollback}; retained at {}",
                    archive.display()
                )
            })?;
        }
        return Err(error.to_string());
    }
    *graph = next;
    drop(graph);
    state.business_events.workspace_list_changed();
    crate::workspace_history::handle("forget", path.to_str())
        .unwrap()
        .map_err(|error| {
            format!("file archived and association removed, but history update failed: {error}")
        })?;
    Ok(Value::Null)
}

pub(crate) fn handle(state: &AppState, request: Request) -> Result<Value, String> {
    if matches!(request.operation.as_str(), "snapshot-restore" | "snapshot-list" | "snapshot-get" | "snapshot-save" | "snapshot-delete" | "snapshot-pin" | "snapshot-rename") {
        return crate::workspace_history::snapshot(state, &request);
    }
    if matches!(request.operation.as_str(), "record-recent" | "record-open") {
        let path = request.path.as_deref().ok_or("missing path")?;
        if !state.fs_scope.is_allowed(path) || ridge_core::dispatch::path_has_traversal(path) {
            return Err("workspace history path outside authorized roots".into());
        }
    }
    if let Some(result) =
        crate::workspace_history::handle(&request.operation, request.path.as_deref())
    {
        return result;
    }
    let checked = match request.operation.as_str() {
        "save" | "auto-save" | "schedule-save" => Some(target(
            request.path.as_deref(),
            request.name.as_deref().unwrap_or("workspace"),
        )?),
        "open" => Some(PathBuf::from(
            request.path.as_deref().ok_or("missing path")?,
        )),
        "list" => Some(default_dir()),
        _ => None,
    };
    if let Some(path) = checked {
        if ridge_core::dispatch::path_has_traversal(&path.to_string_lossy())
            || !state.fs_scope.is_allowed(&path.to_string_lossy())
        {
            return Err("workspace file outside authorized roots".into());
        }
    }
    match request.operation.as_str() {
        "save-restore" => {
            let pending = {
                let graph = state
                    .workspaces
                    .lock()
                    .map_err(|_| "workspace state unavailable")?;
                graph
                    .workspace_ids()
                    .filter_map(|id| {
                        graph.workspace(*id).and_then(|metadata| {
                            let name = metadata.name.as_deref()?.trim();
                            if name.is_empty() {
                                return None;
                            }
                            metadata.file.as_ref().map(|file| PathBuf::from(&file.path))
                        })
                    })
                    .collect::<Vec<_>>()
            };
            let paths = pending
                .into_iter()
                .filter(|path| path.is_file() && file::read(path).is_ok())
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            crate::workspace_history::restore_paths(&paths)?;
            Ok(json!(paths))
        }
        "snapshot" => {
            let graph = state
                .workspaces
                .lock()
                .map_err(|_| "workspace state unavailable")?;
            let metadata = graph
                .workspace(request.workspace_id.ok_or("missing workspaceId")?)
                .ok_or("workspace unavailable")?;
            let association = metadata.file.clone().unwrap_or_default();
            serde_json::to_value(RidgeFile {
                foreign_panes:metadata.foreign_panes.clone(),
                version: RIDGE_FILE_VERSION,
                name: metadata
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("工作区 {}", metadata.display_seq)),
                saved_at: chrono::Utc::now().to_rfc3339(),
                pane_tree: serde_json::to_value(&metadata.pane_tree)
                    .map_err(|error| error.to_string())?,
                git_repos: association.git_repos,
                index_path: association.index_path,
                pane_titles: association.pane_titles,
            })
            .map_err(|error| error.to_string())
        }
        "delete-file" => detach_file(state, &request, false),
        "delete-saved" => detach_file(state, &request, true),
        "save" => save(state, &request, true),
        "auto-save" => save(state, &request, false),
        "schedule-save" => {
            crate::workspace_autosave::schedule(state.clone(), request).map(|_| Value::Null)
        }
        "open" => open(state, request.path.as_deref().ok_or("missing path")?),
        "default-directory" => Ok(json!(default_dir())),
        "info" => {
            if let Some(error) =
                crate::workspace_autosave::error(request.workspace_id.ok_or("missing workspaceId")?)
            {
                return Err(error);
            }
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
        "saved-list" => {
            let directory = default_dir();
            if !directory.exists() {
                return Ok(json!([]));
            }
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let path = entry.path();
                if !entry.file_type().map_err(|error| error.to_string())?.is_file()
                    || !path.extension().and_then(|value| value.to_str())
                        .is_some_and(|value| value.eq_ignore_ascii_case("ridge"))
                    || !state.fs_scope.is_allowed(&path.to_string_lossy())
                {
                    continue;
                }
                let name = path.file_stem().and_then(|value| value.to_str())
                    .map(str::to_string).unwrap_or_else(|| path.to_string_lossy().into_owned());
                let mtime = entry.metadata().and_then(|metadata| metadata.modified()).ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs()).unwrap_or(0);
                entries.push(json!({"name":name,"path":path,"mtime_secs":mtime}));
            }
            entries.sort_by_key(|entry| std::cmp::Reverse(entry["mtime_secs"].as_u64().unwrap_or(0)));
            Ok(json!(entries))
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

/// Existing RPC methods share the same file owner as the private/native adapters.
pub(crate) fn dispatch(
    state: &AppState,
    method: &str,
    args: &Value,
    ctx: &ridge_core::Ctx,
) -> Option<ridge_core::CoreResult<Value>> {
    let operation = match method {
        "get_last_opened_workspace_path" => "last",
        "list_recent_workspaces" => "recent",
        "clear_recent_workspaces" => "clear-recent",
        "clear_last_opened_workspace_path" => "clear-last",
        "delete_workspace_file" => "delete-file",
        "delete_saved_workspace_file" => "delete-saved",
        "get_restore_set" => "restore",
        "list_saved_workspace_files" => "saved-list",
        _ => return None,
    };
    let workspace_id = args["workspaceId"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok());
    if operation == "delete-file" {
        let allowed = (|| -> Result<bool, String> {
            let graph = state
                .workspaces
                .lock()
                .map_err(|_| "workspace state unavailable")?;
            let metadata = graph
                .workspace(workspace_id.ok_or("invalid workspaceId")?)
                .ok_or("workspace unavailable")?;
            Ok(metadata
                .file
                .as_ref()
                .is_none_or(|file| ctx.capabilities().root_scope().is_allowed(&file.path)))
        })();
        match allowed {
            Ok(true) => {}
            Ok(false) => return Some(Err(ridge_core::CoreError::OutsideSandbox)),
            Err(error) => return Some(Err(ridge_core::CoreError::InvalidArgs(error))),
        }
    }
    Some(
        handle(
            state,
            Request {
                operation: operation.into(),
                workspace_id,
                name: None,
                path: args["path"].as_str().map(str::to_string),
            },
        )
        .map(|value| {
            if operation == "saved-list" {
                json!(value.as_array().into_iter().flatten().filter(|entry|
                    entry["path"].as_str().is_some_and(|path|
                        ctx.capabilities().root_scope().is_allowed(path))).collect::<Vec<_>>())
            } else if matches!(operation, "recent" | "restore") {
                json!(value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|path| path
                        .as_str()
                        .is_some_and(|path| state.fs_scope.is_allowed(path)
                            && ctx.capabilities().root_scope().is_allowed(path)))
                    .collect::<Vec<_>>())
            } else if operation == "last"
                && value.as_str().is_some_and(|path| {
                    !state.fs_scope.is_allowed(path)
                        || !ctx.capabilities().root_scope().is_allowed(path)
                })
            {
                Value::Null
            } else {
                value
            }
        })
        .map_err(ridge_core::CoreError::Io),
    )
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
