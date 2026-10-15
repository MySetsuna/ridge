fn setup_app(
    app: &mut tauri::App,
    teammate_state: AppState,
    event_rx: mpsc::Receiver<GlobalEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(target: "ridge::init", phase = 1, "setup: storing AppHandle");
    if !utils::cdp::isolated() {
        clipboard_image::cleanup_old_temp_images(std::time::Duration::from_secs(3600));
    }
    let handle = app.handle().clone();
    lsp::set_app_handle(handle.clone());
    let _ = teammate_state.app_handle.set(handle.clone());
    match teammate::memory::runtime_directory() {
        Ok(directory)=>teammate::memory::init_dir(directory),
        Err(error)=>tracing::warn!(%error,"Runtime memory namespace unavailable; persistence disabled"),
    }
    build_ridge_window(app.handle(), "main", true)?;
    start_kernel_bootstrap(app);
    if let Err(error) = crate::tray::build_tray(app) {
        tracing::error!(target: "ridge::tray", error = %error, "tray init failed");
    }
    crate::taskbar::refresh_jump_list_async(handle.clone());
    register_deep_links(app);
    spawn_event_forwarder(handle, event_rx);
    Ok(())
}

