//! Transitional Cloud transport forwards to the same Runtime Host as LAN.
use crate::state::AppState;
use uuid::Uuid;
#[tauri::command]
pub fn bind_remote_terminal(client_id:String,workspace_id:String,pane_id:String,activation_id:u64,active:bool)->Result<(),String> {
    crate::commands::runtime::request("bind_remote_terminal",serde_json::json!({"clientId":client_id,"workspaceId":workspace_id,"paneId":pane_id,"activationId":activation_id,"active":active}),true,crate::commands::runtime::window_connection("cloud-transport"))
}
#[tauri::command]
pub async fn dispatch_remote_invoke(method:String,args:serde_json::Value,client_id:Option<String>)->Result<serde_json::Value,serde_json::Value> {
    crate::commands::runtime::request_result("dispatch_remote_invoke",serde_json::json!({"method":method,"args":args,"clientId":client_id}),true,crate::commands::runtime::window_connection("cloud-transport"))
}
pub(crate) fn broadcast_pane_resize(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    rows: u16,
    cols: u16,
    owner: crate::types::PaneResizeOwner,
) {
    let rows = rows.max(1).min(500);
    let cols = cols.max(1).min(500);
    state.broadcast_remote_event(
        workspace_id,
        pane_id,
        crate::types::RemotePtyEvent::PtyResized {
            workspace_id,
            pane_id,
            rows,
            cols,
            owner,
        },
    );
    if let Some(app) = state.app_handle.get() {
        let payload = serde_json::json!({
            "workspaceId": workspace_id.to_string(),
            "paneId": pane_id.to_string(),
            "rows": rows,
            "cols": cols,
            "owner": owner.as_str(),
        });
        use tauri::Emitter;
        let _ = app.emit("pty-resized", &payload);
    }
}
