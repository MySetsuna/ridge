//! One owner for the existing last/recent/restore files; legacy locations and JSON stay portable.
use ridge_core::workspace::file::write_atomic;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
static WRITES: Mutex<()> = Mutex::new(());
fn same_path(a: &str, b: &str) -> bool {
    let normalize = |path: &str| {
        let path = path.replace('\\', "/");
        let path = path
            .strip_prefix("//?/")
            .unwrap_or(&path)
            .trim_end_matches('/')
            .to_string();
        if cfg!(windows) {
            path.to_lowercase()
        } else {
            path
        }
    };
    normalize(a) == normalize(b)
}
pub(crate) fn directory() -> PathBuf {
    if std::env::var_os("RIDGE_KERNEL_DATA_DIR").is_some() {
        return crate::registry::ridge_data_dir();
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.tauri-app.ridge")
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn list(name: &str) -> Result<Vec<String>, String> {
    read(&directory().join(name))?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|error| format!("invalid {name}; original preserved: {error}"))
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}
fn last() -> Result<Option<String>, String> {
    read(&directory().join("last_workspace.txt"))?
        .map(|bytes| {
            String::from_utf8(bytes)
                .map(|text| text.trim().to_string())
                .map_err(|error| error.to_string())
        })
        .transpose()
        .map(|value| value.filter(|text| !text.is_empty()))
}
pub(crate) fn write(name: &str, bytes: &[u8]) -> Result<(), String> {
    write_at(&directory().join(name), bytes)
}
pub(crate) fn write_at(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(previous) = read(&path)? {
        write_atomic(
            &path.with_extension(format!("archive-{}", uuid::Uuid::new_v4())),
            &previous,
        )
        .map_err(|error| error.to_string())?;
    }
    write_atomic(&path, bytes).map_err(|error| error.to_string())
}
pub(crate) fn restore_paths(paths: &[String]) -> Result<(), String> {
    let _guard = WRITES.lock().map_err(|_| "workspace history unavailable")?;
    write(
        "restore_workspaces.json",
        &serde_json::to_vec_pretty(paths).map_err(|error| error.to_string())?,
    )
}
pub(crate) fn record(path: &str, opened: bool) -> Result<(), String> {
    let _guard = WRITES.lock().map_err(|_| "workspace history unavailable")?;
    let mut recent = list("recent_workspaces.json")?;
    recent.retain(|existing| existing != path);
    recent.insert(0, path.into());
    recent.truncate(10);
    write(
        "recent_workspaces.json",
        &serde_json::to_vec_pretty(&recent).map_err(|error| error.to_string())?,
    )?;
    if opened {
        write("last_workspace.txt", path.as_bytes())?;
    }
    Ok(())
}
pub(crate) fn handle(operation: &str, path: Option<&str>) -> Option<Result<Value, String>> {
    Some(match operation {
        "last" => last().map(|value| json!(value)),
        "recent" => list("recent_workspaces.json").map(|paths| {
            json!(paths
                .into_iter()
                .filter(|path| Path::new(path).is_file())
                .collect::<Vec<_>>())
        }),
        "restore" => list("restore_workspaces.json").map(|paths| {
            json!(paths
                .into_iter()
                .filter(|path| Path::new(path).is_file())
                .collect::<Vec<_>>())
        }),
        "clear-recent" | "clear-last" => (|| {
            let _guard = WRITES.lock().map_err(|_| "workspace history unavailable")?;
            if operation == "clear-last" {
                write("last_workspace.txt", b"")?;
            } else {
                write("recent_workspaces.json", b"[]")?;
            }
            Ok(Value::Null)
        })(),
        "record-recent" | "record-open" => path
            .ok_or_else(|| "missing path".to_string())
            .and_then(|path| record(path, operation == "record-open"))
            .map(|_| Value::Null),
        "forget" => (|| {
            let path = path.ok_or("missing path")?;
            let _guard = WRITES.lock().map_err(|_| "workspace history unavailable")?;
            let mut recent = list("recent_workspaces.json")?;
            let previous_last = last()?;
            recent.retain(|existing| !same_path(existing, path));
            write(
                "recent_workspaces.json",
                &serde_json::to_vec_pretty(&recent).map_err(|error| error.to_string())?,
            )?;
            if previous_last
                .as_deref()
                .is_some_and(|existing| same_path(existing, path))
            {
                write("last_workspace.txt", b"")?;
            }
            Ok(Value::Null)
        })(),
        _ => return None,
    })
}

/// Existing workspace_history.json schema; raw objects retain unknown fields.
pub(crate) fn snapshot(state: &crate::server::AppState, request: &crate::workspace_files::Request) -> Result<Value, String> {
    snapshot_scoped(state,request,None)
}
fn snapshot_scoped(state: &crate::server::AppState, request: &crate::workspace_files::Request, scope: Option<&ridge_core::sandbox::RootScope>) -> Result<Value,String> {
    let operation = request.operation.as_str();
    let saved = if operation == "snapshot-save" {
        let graph = state.workspaces.lock().map_err(|_| "workspace state unavailable")?;
        let metadata = graph.workspace(request.workspace_id.ok_or("missing workspaceId")?).ok_or("workspace unavailable")?;
        if metadata.pane_tree.panes.values().filter_map(|pane|pane.cwd.as_ref()).any(|cwd|
            !state.fs_scope.is_allowed(&cwd.to_string_lossy()) || scope.is_some_and(|scope|!scope.is_allowed(&cwd.to_string_lossy()))) {
            return Err("historical workspace outside authorized roots".into());
        }
        Some(json!({"id":uuid::Uuid::new_v4(),"workspace_id":request.workspace_id,"name":request.name.clone().or_else(||metadata.name.clone()).unwrap_or_else(||format!("Saved Workspace {}",chrono::Utc::now().format("%Y-%m-%d %H:%M"))),
            "saved_at":chrono::Utc::now().to_rfc3339(),"pane_count":metadata.pane_tree.get_all_leaves().len(),"is_pinned":false,
            "foreign_panes":metadata.foreign_panes,"pane_tree_json":serde_json::to_string(&metadata.pane_tree).map_err(|error|error.to_string())?}))
    } else {None};
    let _guard = WRITES.lock().map_err(|_| "workspace history unavailable")?;
    let mut store: Value = match read(&directory().join("workspace_history.json"))? {
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|error|format!("invalid workspace history; original preserved: {error}"))?,
        None => json!({"items":[]}),
    };
    let items = store.get_mut("items").and_then(Value::as_array_mut).ok_or("invalid workspace history; original preserved")?;
    if items.iter().any(|item| !item["id"].is_string() || !item["name"].is_string() || !item["saved_at"].is_string()
        || !item["pane_count"].is_u64() || !item["is_pinned"].is_boolean() || !item["pane_tree_json"].is_string()) {
        return Err("invalid workspace history; original preserved".into());
    }
    if operation == "snapshot-list" {return Ok(json!(items));}
    if let Some(item) = saved {
        let id = item["id"].clone();
        items.push(item);
        write("workspace_history.json", &serde_json::to_vec_pretty(&store).map_err(|error|error.to_string())?)?;
        state.business_events.workspace_history_changed();
        return Ok(id);
    }
    let id = request.path.as_deref().ok_or("missing historyId")?;
    let index = items.iter().position(|item|item["id"] == id).ok_or("workspace history not found")?;
    if scope.is_some_and(|scope|!snapshot_allowed(state,&items[index],scope)) {
        return Err("historical workspace outside authorized roots".into());
    }
    if operation == "snapshot-restore" {
        let item = items[index].clone();
        drop(_guard);
        return restore_snapshot(state, &item);
    }
    match operation {
        "snapshot-get" => return Ok(items[index].clone()),
        "snapshot-delete" => {items.remove(index);},
        "snapshot-pin" => items[index]["is_pinned"] = json!(!items[index]["is_pinned"].as_bool().unwrap()),
        "snapshot-rename" => items[index]["name"] = json!(request.name.as_deref().ok_or("missing name")?),
        _ => return Err("unsupported workspace history operation".into()),
    }
    write("workspace_history.json", &serde_json::to_vec_pretty(&store).map_err(|error|error.to_string())?)?;
    state.business_events.workspace_history_changed();
    Ok(Value::Null)
}

fn restore_snapshot(state: &crate::server::AppState, item: &Value) -> Result<Value, String> {
    use ridge_core::workspace::{file::{self,RidgeFile,RIDGE_FILE_VERSION},graph::WorkspaceMeta,pane_tree::PaneTree};
    let tree_value: Value = serde_json::from_str(item["pane_tree_json"].as_str().ok_or("missing pane tree")?).map_err(|error|error.to_string())?;
    let foreign_panes:std::collections::HashMap<uuid::Uuid,ridge_core::workspace::graph::ForeignPaneBinding>=serde_json::from_value(item.get("foreign_panes").cloned().unwrap_or_else(||json!({}))).map_err(|error|error.to_string())?;
    file::validate(&RidgeFile {foreign_panes:foreign_panes.clone(),version:RIDGE_FILE_VERSION,name:item["name"].as_str().unwrap().into(),saved_at:item["saved_at"].as_str().unwrap().into(),pane_tree:tree_value.clone(),git_repos:Vec::new(),index_path:None,pane_titles:Default::default()})?;
    let tree: PaneTree = serde_json::from_value(tree_value).map_err(|error|error.to_string())?;
    if tree.panes.values().filter_map(|pane|pane.cwd.as_ref()).any(|cwd| !state.fs_scope.is_allowed(&cwd.to_string_lossy())) {
        return Err("historical workspace outside authorized roots".into());
    }
    if item.get("workspace_id").is_some_and(|id|!id.is_null() && !id.is_string()) {
        return Err("invalid historical workspace identity; original preserved".into());
    }
    let bound = item["workspace_id"].as_str().map(uuid::Uuid::parse_str).transpose().map_err(|error|error.to_string())?;
    let mut retired = state.retired_workspaces.lock().map_err(|_|"workspace retirement unavailable")?;
    let mut graph = state.workspaces.lock().map_err(|_|"workspace state unavailable")?;
    if let Some(id) = bound {
        if let Some(metadata) = graph.workspace(id) {
            if metadata.foreign_panes!=foreign_panes || metadata.pane_tree.panes.len()!=tree.panes.len() || tree.panes.keys().any(|pane|!metadata.pane_tree.panes.contains_key(pane)) {
                return Err("historical workspace conflicts with current identity; original preserved".into());
            }
            return Ok(json!({"workspaceId":id,"alreadyOpen":true}));
        }
    }
    if graph.workspace_ids().any(|id|graph.workspace(*id).is_some_and(|metadata|
        tree.panes.keys().any(|pane|metadata.pane_tree.panes.contains_key(pane)))) {
        return Err("historical pane identities belong to another workspace; original preserved".into());
    }
    let id = bound.unwrap_or_else(uuid::Uuid::new_v4);
    let mut next=graph.clone();
    next.insert_workspace(id,WorkspaceMeta {foreign_panes,pane_tree:tree,name:Some(item["name"].as_str().unwrap().into()),..Default::default()});
    crate::registry::save_workspace_graph_at(&state.workspaces_path,&next).map_err(|error|error.to_string())?;
    *graph=next;
    retired.remove(&id);
    drop(graph);
    drop(retired);
    state.business_events.workspace_list_changed();
    state.business_events.pane_tree_changed(id);
    Ok(json!({"workspaceId":id,"alreadyOpen":false}))
}

pub(crate) fn dispatch(state: &crate::server::AppState, method: &str, args: &Value, ctx: &ridge_core::Ctx) -> Option<ridge_core::CoreResult<Value>> {
    let operation=match method {
        "list_saved_workspaces"|"list_workspace_history"=>"snapshot-list",
        "save_workspace"=>"snapshot-save",
        "restore_workspace"=>"snapshot-restore",
        "delete_saved_workspace"|"delete_workspace_history"=>"snapshot-delete",
        "rename_saved_workspace"|"rename_workspace_history"=>"snapshot-rename",
        "toggle_pin_workspace_history"=>"snapshot-pin",
        _=>return None,
    };
    let request=crate::workspace_files::Request {operation:operation.into(),workspace_id:args["workspaceId"].as_str().and_then(|id|uuid::Uuid::parse_str(id).ok()),
        path:args["historyId"].as_str().map(str::to_string),name:args["name"].as_str().map(str::to_string)};
    let allowed=|item:&Value| snapshot_allowed(state,item,ctx.capabilities().root_scope());
    Some((|| {
        if operation=="snapshot-save" {
            let graph=state.workspaces.lock().map_err(|_|ridge_core::CoreError::Io("workspace state unavailable".into()))?;
            let metadata=graph.workspace(request.workspace_id.ok_or_else(||ridge_core::CoreError::InvalidArgs("missing workspaceId".into()))?).ok_or_else(||ridge_core::CoreError::InvalidArgs("workspace unavailable".into()))?;
            if !allowed(&json!({"pane_tree_json":serde_json::to_string(&metadata.pane_tree).map_err(|error|ridge_core::CoreError::Io(error.to_string()))?})) {return Err(ridge_core::CoreError::OutsideSandbox);}
        } else if operation!="snapshot-list" {
            let mut read=request.clone();read.operation="snapshot-get".into();
            let item=snapshot(state,&read).map_err(ridge_core::CoreError::Io)?;
            if !allowed(&item) {return Err(ridge_core::CoreError::OutsideSandbox);}
        }
        let value=snapshot_scoped(state,&request,Some(ctx.capabilities().root_scope())).map_err(ridge_core::CoreError::Io)?;
        Ok(if operation=="snapshot-list" {json!(value.as_array().into_iter().flatten().filter(|item|allowed(item)).collect::<Vec<_>>())}
            else if operation=="snapshot-restore" {value["workspaceId"].clone()} else {value})
    })())
}

fn snapshot_allowed(state:&crate::server::AppState,item:&Value,scope:&ridge_core::sandbox::RootScope)->bool {
    if state.fs_scope.is_unrestricted() && scope.is_unrestricted() {return true;}
    item["pane_tree_json"].as_str().and_then(|tree|serde_json::from_str::<ridge_core::workspace::pane_tree::PaneTree>(tree).ok()).is_some_and(|tree|
        tree.panes.values().filter_map(|pane|pane.cwd.as_ref()).all(|cwd|state.fs_scope.is_allowed(&cwd.to_string_lossy()) && scope.is_allowed(&cwd.to_string_lossy())))
}

#[cfg(all(test, windows))]
mod persistence_tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn denied_replace_preserves_legacy_bytes_and_archive_then_allows_retry() {
        let root = std::env::temp_dir().join(format!("ridge-history-conflict-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("runtime-settings.json");
        let legacy = br#"{"legacyPreference":"retained","defaultCwd":null}"#;
        let next = br#"{"legacyPreference":"retained","defaultCwd":"C:/fixture"}"#;
        std::fs::write(&path, legacy).unwrap();
        // Allow reading the original for backup, but deny write/delete sharing.
        let occupied = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        assert!(write_at(&path, next).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        let entries: Vec<_> = std::fs::read_dir(&root).unwrap().map(Result::unwrap).collect();
        let archives: Vec<_> = entries.iter().filter(|entry| entry.file_name().to_string_lossy().contains("archive-")).collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(std::fs::read(archives[0].path()).unwrap(), legacy);
        assert!(!entries.iter().any(|entry| entry.file_name().to_string_lossy().contains("tmp-")));
        drop(occupied);
        write_at(&path, next).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), next);
        let restored = root.join("restored.json");
        std::fs::copy(archives[0].path(), &restored).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(restored).unwrap()).unwrap();
        assert_eq!(value["legacyPreference"], "retained");
        assert!(value["defaultCwd"].is_null());
    }
}
