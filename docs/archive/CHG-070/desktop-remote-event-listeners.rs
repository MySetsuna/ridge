fn register_remote_event_listeners(app: &tauri::App, handle: &tauri::AppHandle) {
    use tauri::Listener;
    for name in [
        "teammate-layout-changed",
        "teammate-active-pane-changed",
    ] {
        let forward_handle = handle.clone();
        app.listen_any(name, move |event| {
            let payload = serde_json::from_str(event.payload()).unwrap_or(serde_json::Value::Null);
            crate::remote_bridge::forward_event(&forward_handle, name, payload);
        });
    }
}

