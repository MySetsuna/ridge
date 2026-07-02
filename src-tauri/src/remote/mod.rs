// LAN 远控鉴权原语已迁入共享 crate ridge-remote（零 Tauri 依赖）。
// 重导出以保持 `super::auth::*` / `crate::remote::auth::*` 引用不变。
pub use ridge_remote::auth;
pub mod core_bridge;
// mDNS discovery broadcast: moved to shared crate ridge-remote (zero Tauri deps).
// Re-exported so `super::mdns::*` / `crate::remote::mdns::*` references keep working.
pub use ridge_remote::mdns;
mod server;
// TLS cert generation: moved to shared crate ridge-remote.
// Re-exported so super::tls::resolve_config etc. keep working in server.rs.
pub use ridge_remote::tls;
// LAN IP detection: moved to shared crate ridge-remote::net (zero Tauri deps).
// Re-exported so `super::detect_lan_ip(s)` / `crate::remote::detect_lan_ip(s)`
// references (server.rs / commands / bin) keep working.
pub use ridge_remote::net::{detect_lan_ip, detect_lan_ips};

pub use server::spawn_remote_server;

/// Forward a Tauri event to all connected desktop-browser remote clients (the
/// "desktop UI in a browser" mode), so the browser's `listen()` shim dispatches
/// it exactly like a native event. Add a call next to any `app.emit(...)` whose
/// event the desktop UI subscribes to. No-op when AppState isn't managed.
pub fn forward_event<S: serde::Serialize>(app: &tauri::AppHandle, name: &str, payload: S) {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    let _ = state.remote_ui_event_tx.send(crate::types::RemoteUiEvent {
        name: name.to_string(),
        payload: value,
    });
}
