#[tauri::command]
pub fn set_remote_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    let current = state.remote_enabled.load(Ordering::Acquire);
    let has_server = *state.remote_port.read() != 0;
    let state_matches = if enabled {
        current && has_server
    } else {
        !current && !has_server
    };
    if state_matches {
        return Ok(());
    }

    if enabled {
        start_remote_server(&state)?;
    } else {
        stop_in_process_remote_server(&state);
    }

    tracing::info!(target: "ridge::remote", enabled, "Remote control toggle changed");
    Ok(())
}

#[tauri::command]
pub fn get_remote_enabled(state: State<AppState>) -> Result<bool, String> {
    Ok(state.remote_enabled.load(Ordering::Acquire))
}

/// §sessions: list the currently-connected remote control sessions for the
/// desktop RemotePanel (IP + device id + connected duration).
#[tauri::command]
pub fn list_remote_sessions(state: State<AppState>) -> Vec<serde_json::Value> {
    let now = std::time::SystemTime::now();
    let mut sessions: Vec<serde_json::Value> = state
        .remote_client_registry
        .list()
        .into_iter()
        .map(|c| {
            let secs = now
                .duration_since(c.connected_at)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            serde_json::json!({
                "id": c.id,
                "remoteAddr": c.remote_addr,
                "deviceId": c.device_id,
                "userAgent": c.user_agent,
                "connectedSecs": secs,
            })
        })
        .collect();
    sessions.sort_by_key(|v| v["id"].as_u64().unwrap_or(0));
    sessions
}

/// §sessions: force-disconnect a session — invalidate its session token (so the
/// device must re-enter the auth code to reconnect; NOT blacklisted) then kick
/// the live WebSocket. The 1s health check closes it promptly.
#[tauri::command]
pub fn disconnect_session(state: State<AppState>, id: u64) -> bool {
    if let Some(info) = state.remote_client_registry.info_of(id) {
        if let Some(ref t) = info.token {
            state.remote_session_store.invalidate(t);
        }
    }
    state.remote_client_registry.kick(id)
}

/// §blacklist: bar a live session's device — record its device id (+ IP) in the
/// persistent blacklist, invalidate its token, and kick it. Until removed from
/// the blacklist it can no longer obtain a token or connect.
#[tauri::command]
pub fn add_to_blacklist(state: State<AppState>, id: u64) -> bool {
    let Some(info) = state.remote_client_registry.info_of(id) else {
        return false;
    };
    if let Some(ref t) = info.token {
        state.remote_session_store.invalidate(t);
    }
    let device_id = if info.device_id.is_empty() {
        None
    } else {
        Some(info.device_id.clone())
    };
    let ip = if info.remote_addr.is_empty() || info.remote_addr == "unknown" {
        None
    } else {
        Some(info.remote_addr.clone())
    };
    // Need at least one identity to enforce against.
    if device_id.is_none() && ip.is_none() {
        return false;
    }
    let label = device_id
        .as_deref()
        .map(|d| d.chars().take(8).collect::<String>())
        .unwrap_or_else(|| info.remote_addr.clone());
    let added_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state.remote_blacklist.add(crate::state::BlacklistEntry {
        id: uuid::Uuid::new_v4().to_string(),
        device_id,
        ip,
        label,
        added_at,
    });
    state.remote_client_registry.kick(id)
}

/// §blacklist: list current blacklist entries for the RemotePanel.
#[tauri::command]
pub fn list_blacklist(state: State<AppState>) -> Vec<crate::state::BlacklistEntry> {
    state.remote_blacklist.list()
}

/// §blacklist: remove an entry by its id (un-ban). The device can then reconnect
/// via the auth code.
#[tauri::command]
pub fn remove_from_blacklist(state: State<AppState>, id: String) -> bool {
    state.remote_blacklist.remove(&id)
}

fn start_remote_server(state: &AppState) -> Result<(), String> {
    start_in_process_remote_server(state)
}

fn remote_transport_state(state: &AppState) -> (u16, bool) {
    let port = *state.remote_port.read();
    let enabled = state.remote_enabled.load(Ordering::Acquire);
    remote_transport_state_values(port, enabled)
}

fn remote_transport_state_values(port: u16, enabled: bool) -> (u16, bool) {
    if port == 0 {
        (0, false)
    } else {
        (port, enabled)
    }
}

/// Stop the in-process LAN transport on the explicit full-quit path.
pub fn stop_remote_server(state: &AppState) {
    stop_in_process_remote_server(state);
    tracing::info!(target: "ridge::remote", "explicit full quit stopped in-process Remote transport");
}

fn start_in_process_remote_server(state: &AppState) -> Result<(), String> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let auth = state.remote_auth.clone();
    let handle = crate::remote_bridge::spawn_remote_server(state.clone(), auth, shutdown_rx)
        .ok_or_else(|| "Failed to bind remote server port".to_string())?;

    // Start mDNS broadcast so mobile clients can discover the server.
    let (mdns_handle, mdns_stop) = mdns::spawn_mdns_broadcast(handle.port);

    *state.remote_port.write() = handle.port;
    *state.remote_thread.lock() = Some(handle.thread);
    *state.remote_shutdown.lock() = Some(shutdown_tx);
    *state.remote_mdns.lock() = Some((mdns_handle, mdns_stop));
    state.remote_enabled.store(true, Ordering::Release);

    // In dev mode, spawn the Vite dev server for the remote app
    if cfg!(debug_assertions) {
        let current_dir = std::env::current_dir().expect("failed to get current dir");
        let project_root = if current_dir.ends_with("src-tauri") {
            current_dir.parent().unwrap().to_path_buf()
        } else {
            current_dir
        };

        tracing::info!(target: "ridge::remote", "Spawning Vite in Root: {:?}", project_root);

        // 使用 npx 直接调用 vite，避开 pnpm 脚本的 shell 问题
        let mut cmd = std::process::Command::new(if cfg!(target_os = "windows") {
            "npx.cmd"
        } else {
            "npx"
        });
        cmd.args(["vite", "dev", "--config", "vite.remote.config.js"]);

        match cmd
            .current_dir(&project_root)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(child) => {
                tracing::info!(target: "ridge::remote", "Vite spawned, PID: {}", child.id());
                *state.remote_dev_process.lock() = Some(child);
            }
            Err(e) => {
                tracing::error!(target: "ridge::remote", "Failed to spawn Vite: {:?}", e);
            }
        }
    }

    Ok(())
}

fn stop_in_process_remote_server(state: &AppState) {
    if let Some(tx) = state.remote_shutdown.lock().take() {
        let _ = tx.send(());
    }
    if let Some(thread) = state.remote_thread.lock().take() {
        let _ = thread.join();
    }
    // Stop mDNS broadcast.
    if let Some((handle, stop_flag)) = state.remote_mdns.lock().take() {
        stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = handle.join();
    }
    // Clean up dev process.
    if cfg!(debug_assertions) {
        if let Some(mut child) = state.remote_dev_process.lock().take() {
            match child.try_wait() {
                Ok(None) => {
                    let pid = child.id();
                    #[cfg(target_os = "windows")]
                    {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .status();
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let _ = child.kill();
                    }
                }
                Ok(Some(status)) => {
                    tracing::debug!(target: "ridge::remote", %status, "Vite dev process already exited");
                }
                Err(e) => {
                    tracing::warn!(target: "ridge::remote", "Vite process status check failed: {:?}", e);
                }
            }
        }
    }
    *state.remote_port.write() = 0;
    state.remote_enabled.store(false, Ordering::Release);
    tracing::info!(target: "ridge::remote", "Remote control server stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_or_missing_host_cannot_report_ready_from_cached_state() {
        assert_eq!(remote_transport_state_values(0, false), (0, false));
    }

    #[test]
    fn live_host_state_is_used_for_port_and_enabled_flag() {
        assert_eq!(remote_transport_state_values(9527, true), (9527, true));
        assert_eq!(remote_transport_state_values(9527, false), (9527, false));
    }
}
