fn hide_desktop<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    crate::commands::ridge_file::save_restore_set(app, &app.state::<AppState>());
    prepare_for_hide(&window);
    match window.hide() {
        Ok(()) => {
            tracing::info!(target: "ridge::tray", "desktop UI hidden; kernel and remote remain running")
        }
        Err(error) => {
            tracing::warn!(target: "ridge::tray", error = %error, "hide desktop shell failed")
        }
    }
}

