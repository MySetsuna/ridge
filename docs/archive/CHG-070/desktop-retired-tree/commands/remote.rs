use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

#[tauri::command]
pub fn get_remote_info()->Result<serde_json::Value,String> {auth_request("get_remote_info",serde_json::json!({}))}

/// §leak-trace (temporary diagnostic): manually reconcile every workspace's PTYs
/// to its pane_tree leaves, returning the count reaped. Lets the e2e harness
/// trigger reaping deterministically over CDP, independent of the WS list-panes
/// path or any other client. In-process only.
#[tauri::command]
pub async fn remote_reap_orphans(state: State<'_, AppState>) -> Result<usize, String> {
    Ok(crate::commands::terminal::reap_orphan_panes_all(&*state).await)
}

/// Kernel owns TOTP and trust state; transport shells never load a seed.
fn auth_request<T: serde::de::DeserializeOwned>(method:&str,args:serde_json::Value)->Result<T,String> {
    super::runtime::request(method,args,true,super::runtime::window_connection("remote-auth"))
}
#[tauri::command]
pub fn verify_remote_totp(code:&str)->Result<bool,String> {
    auth_request("verify_remote_totp",serde_json::json!({"code":code}))
}
#[tauri::command]
pub fn verify_remote_totp_bind(transcript:Vec<u8>,tag:Vec<u8>)->Result<bool,String> {
    auth_request("verify_remote_totp_bind",serde_json::json!({"transcript":transcript,"tag":tag}))
}
#[tauri::command]
pub fn remote_reset_totp(app:AppHandle)->Result<(),String> {
    auth_request::<()>("remote_reset_totp",serde_json::json!({}))?;
    let _=app.emit("remote-totp-changed",()); Ok(())
}
#[tauri::command]
pub fn remote_set_totp_identity(app:AppHandle,username:Option<String>)->Result<(),String> {
    auth_request::<()>("remote_set_totp_identity",serde_json::json!({"username":username}))?;
    let _=app.emit("remote-totp-changed",()); Ok(())
}
#[tauri::command]
pub fn totp_trust_check(ctrl_pub_b64:&str)->Result<bool,String> {
    auth_request("totp_trust_check",serde_json::json!({"ctrlPubB64":ctrl_pub_b64}))
}
#[tauri::command]
pub fn totp_trust_record(ctrl_pub_b64:&str)->Result<(),String> {
    auth_request("totp_trust_record",serde_json::json!({"ctrlPubB64":ctrl_pub_b64}))
}
#[tauri::command]
pub fn totp_trust_revoke_all()->Result<(),String> {auth_request("totp_trust_revoke_all",serde_json::json!({}))}

/// 零信任 #2：返回本设备 Ed25519 身份**公钥**（32 字节）。controller 经此公钥验签
/// host 的握手签名并 TOFU 固定指纹。私钥永不离开 Rust 侧（DPAPI/0600）。
#[tauri::command]
pub fn get_device_identity_pub() -> Result<Vec<u8>, String> {
    super::runtime::request("get_device_identity_pub", serde_json::json!({}), true, super::runtime::window_connection("device-identity"))
}

/// Domain-separated device binding signature; private key remains in Runtime.
#[tauri::command]
pub fn sign_device_identity(context: Vec<u8>) -> Result<Vec<u8>, String> {
    super::runtime::request("sign_device_identity", serde_json::json!({"context":context}), true, super::runtime::window_connection("device-identity"))
}

#[tauri::command]
pub fn set_remote_enabled(enabled:bool)->Result<(),String> {auth_request("set_remote_enabled",serde_json::json!({"enabled":enabled}))}
#[tauri::command]
pub fn get_remote_enabled()->Result<bool,String> {auth_request("get_remote_enabled",serde_json::json!({}))}
#[tauri::command]
pub fn list_remote_sessions()->Result<Vec<serde_json::Value>,String> {auth_request("list_remote_sessions",serde_json::json!({}))}
#[tauri::command]
pub fn disconnect_session(id:u64)->Result<bool,String> {auth_request("disconnect_session",serde_json::json!({"id":id}))}
#[tauri::command]
pub fn add_to_blacklist(id:u64)->Result<bool,String> {auth_request("add_to_blacklist",serde_json::json!({"id":id}))}
#[tauri::command]
pub fn list_blacklist()->Result<Vec<crate::state::BlacklistEntry>,String> {auth_request("list_blacklist",serde_json::json!({}))}
#[tauri::command]
pub fn remove_from_blacklist(id:String)->Result<bool,String> {auth_request("remove_from_blacklist",serde_json::json!({"id":id}))}
