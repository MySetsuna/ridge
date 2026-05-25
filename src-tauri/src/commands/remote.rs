use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub port: u16,
    pub totp_code: String,
    pub otpauth_uri: String,
    pub ready: bool,
}

/// Query the current remote-control status.
///
/// Returns the listening port, the current TOTP code, and the otpauth://
/// URI for QR-code display. The frontend calls this periodically to
/// refresh the TOTP code (which rotates every 30 seconds).
#[tauri::command]
pub fn get_remote_info(state: State<'_, AppState>) -> RemoteInfo {
    let port = *state.remote_port.read();
    let code = state.remote_auth.current_code();
    let uri = state.remote_auth.otpauth_uri();
    RemoteInfo {
        port,
        totp_code: code,
        otpauth_uri: uri,
        ready: port > 0,
    }
}
