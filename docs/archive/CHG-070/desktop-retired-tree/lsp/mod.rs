//! Transitional LSP RPC and diagnostic transport; Runtime owns all language servers.
use serde_json::{json, Value};


#[tauri::command]
pub async fn lsp_did_open(workspace_root: String, uri: String, language_id: String, text: String) -> Result<(), String> {
    let args = json!({"workspaceRoot":workspace_root, "uri":uri, "languageId":language_id, "text":text});
    tokio::task::spawn_blocking(move || crate::commands::runtime::request("lsp_did_open", args, true,
        crate::commands::runtime::window_connection("lsp"))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn lsp_did_change(workspace_root: String, uri: String, version: i64, text: String) -> Result<(), String> {
    let args = json!({"workspaceRoot":workspace_root, "uri":uri, "version":version, "text":text});
    tokio::task::spawn_blocking(move || crate::commands::runtime::request("lsp_did_change", args, true,
        crate::commands::runtime::window_connection("lsp"))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn lsp_definition(workspace_root: String, uri: String, line: u32, character: u32) -> Result<Value, String> {
    let args = json!({"workspaceRoot":workspace_root, "uri":uri, "line":line, "character":character});
    tokio::task::spawn_blocking(move || crate::commands::runtime::request("lsp_definition", args, true,
        crate::commands::runtime::window_connection("lsp"))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn lsp_hover(workspace_root: String, uri: String, line: u32, character: u32) -> Result<Value, String> {
    let args = json!({"workspaceRoot":workspace_root, "uri":uri, "line":line, "character":character});
    tokio::task::spawn_blocking(move || crate::commands::runtime::request("lsp_hover", args, true,
        crate::commands::runtime::window_connection("lsp"))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn lsp_references(workspace_root: String, uri: String, line: u32, character: u32) -> Result<Value, String> {
    let args = json!({"workspaceRoot":workspace_root, "uri":uri, "line":line, "character":character});
    tokio::task::spawn_blocking(move || crate::commands::runtime::request("lsp_references", args, true,
        crate::commands::runtime::window_connection("lsp"))).await.map_err(|error| error.to_string())?
}

