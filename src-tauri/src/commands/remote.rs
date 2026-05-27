use std::sync::atomic::Ordering;
use tauri::State;
use sysinfo::System;

use crate::remote::mdns;
use crate::state::AppState;

#[tauri::command]
pub fn get_remote_info(state: State<AppState>) -> Result<serde_json::Value, String> {
    let port = *state.remote_port.read();
    let lan_ip = crate::remote::detect_lan_ip();
    let machine_name = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let (totp_code, otpauth_uri) = state.remote_auth.code_and_uri(&machine_name);
    let enabled = state.remote_enabled.load(Ordering::Relaxed);

    Ok(serde_json::json!({
        "port": port,
        "lanIp": lan_ip,
        "totpCode": totp_code,
        "otpauthUri": otpauth_uri,
        "ready": port > 0 && enabled,
        "remoteEnabled": enabled,
        "devMode": cfg!(debug_assertions),
        "machineName": machine_name,
    }))
}

#[tauri::command]
pub fn set_remote_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    let prev = state.remote_enabled.swap(enabled, Ordering::Relaxed);
    if prev == enabled {
        return Ok(());
    }

    if enabled {
        start_remote_server(&state)?;
    } else {
        stop_remote_server(&state);
    }

    tracing::info!(target: "ridge::remote", enabled, "Remote control toggle changed");
    Ok(())
}

#[tauri::command]
pub fn get_remote_enabled(state: State<AppState>) -> Result<bool, String> {
    Ok(state.remote_enabled.load(Ordering::Relaxed))
}

fn start_remote_server(state: &AppState) -> Result<(), String> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let auth = state.remote_auth.clone();
    let handle = crate::remote::spawn_remote_server(state.clone(), auth, shutdown_rx)
        .ok_or_else(|| "Failed to bind remote server port".to_string())?;

    // Start mDNS broadcast so mobile clients can discover the server.
    let (mdns_handle, mdns_stop) = mdns::spawn_mdns_broadcast(handle.port);

    *state.remote_port.write() = handle.port;
    *state.remote_thread.lock() = Some(handle.thread);
    *state.remote_shutdown.lock() = Some(shutdown_tx);
    *state.remote_mdns.lock() = Some((mdns_handle, mdns_stop));

    // In dev mode, spawn Vite dev server for the mobile app
    if cfg!(debug_assertions) {
        let current_dir = std::env::current_dir().expect("failed to get current dir");
        let project_root = if current_dir.ends_with("src-tauri") {
            current_dir.parent().unwrap().to_path_buf()
        } else {
            current_dir
        };

        tracing::info!(target: "ridge::remote", "Spawning Vite in Root: {:?}", project_root);

        // 使用 npx 直接调用 vite，避开 pnpm 脚本的 shell 问题
        let mut cmd = std::process::Command::new(if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" });
        cmd.args(["vite", "dev", "--config", "vite.mobile.config.js"]);

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

pub fn stop_remote_server(state: &AppState) {
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
    tracing::info!(target: "ridge::remote", "Remote control server stopped");
}
