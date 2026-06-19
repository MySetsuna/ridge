use std::sync::atomic::Ordering;
use base64::Engine as _;
use sysinfo::System;
use tauri::{AppHandle, Emitter, State};

use crate::remote::mdns;
use crate::state::AppState;

#[tauri::command]
pub fn get_remote_info(state: State<AppState>) -> Result<serde_json::Value, String> {
    let port = *state.remote_port.read();
    let lan_ip = crate::remote::detect_lan_ip();
    let lan_ips = crate::remote::detect_lan_ips();
    let machine_name = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let (totp_code, otpauth_uri) = state.remote_auth.code_and_uri(&machine_name);
    let enabled = state.remote_enabled.load(Ordering::Relaxed);

    // §leak-trace (temporary diagnostic): per-workspace pane_tree leaves vs the
    // terminals / pending_spawns maps. An orphan PTY shows up as terminals or
    // pending > leaves. In-process command only — never crosses the /info HTTP
    // boundary, so this exposes no secret.
    let pane_debug: Vec<serde_json::Value> = {
        let map = state.workspaces.read();
        map.iter()
            .map(|(wid, ws)| {
                let leaves: std::collections::HashSet<_> =
                    ws.pane_tree.get_all_leaves().into_iter().collect();
                let orphan_terms: Vec<String> = ws
                    .terminals
                    .keys()
                    .filter(|id| !leaves.contains(id))
                    .map(|id| id.to_string())
                    .collect();
                let orphan_pend: Vec<String> = ws
                    .pending_spawns
                    .keys()
                    .filter(|id| !leaves.contains(id))
                    .map(|id| id.to_string())
                    .collect();
                serde_json::json!({
                    "ws": wid.to_string(),
                    "leaves": leaves.len(),
                    "terminals": ws.terminals.len(),
                    "pending": ws.pending_spawns.len(),
                    "orphanTerminals": orphan_terms,
                    "orphanPending": orphan_pend,
                })
            })
            .collect()
    };

    Ok(serde_json::json!({
        "port": port,
        "lanIp": lan_ip,
        "lanIps": lan_ips,
        "totpCode": totp_code,
        "otpauthUri": otpauth_uri,
        "ready": port > 0 && enabled,
        "remoteEnabled": enabled,
        "devMode": cfg!(debug_assertions),
        "machineName": machine_name,
        "paneDebug": pane_debug,
    }))
}

/// §leak-trace (temporary diagnostic): manually reconcile every workspace's PTYs
/// to its pane_tree leaves, returning the count reaped. Lets the e2e harness
/// trigger reaping deterministically over CDP, independent of the WS list-panes
/// path or any other client. In-process only.
#[tauri::command]
pub async fn remote_reap_orphans(state: State<'_, AppState>) -> Result<usize, String> {
    Ok(crate::commands::terminal::reap_orphan_panes_all(&*state).await)
}

/// §cloud-TOTP (contract §4): verify a controller-supplied 6-digit TOTP code
/// against the host's local `RemoteAuth` (the SAME RFC6238 secret the LAN flow
/// uses) — the ±1 time window is applied inside `RemoteAuth::verify`.
///
/// Used by the cloud host bridge (`cloudHostBridge.ts`) to gate the E2EE data
/// channel: while unverified, business invokes/pane-subscribes are rejected;
/// the controller sends its code over the CONTROL channel, the bridge calls
/// this command, and a `true` result lifts the gate for that connection.
#[tauri::command]
pub fn verify_remote_totp(state: State<AppState>, code: &str) -> bool {
    state.remote_auth.verify(code)
}

/// 零信任 #2：返回本设备 Ed25519 身份**公钥**（32 字节）。controller 经此公钥验签
/// host 的握手签名并 TOFU 固定指纹。私钥永不离开 Rust 侧（DPAPI/0600）。
#[tauri::command]
pub fn get_device_identity_pub(state: State<AppState>) -> Vec<u8> {
    state.device_identity.public_bytes().to_vec()
}

/// 零信任 #2：用设备身份私钥对 id-bind 上下文签名，返回 64 字节 Ed25519 签名。
///
/// 用途**锁死**：Rust 侧强制加固定域分隔前缀 `ridge-id-bind-v1`，故此命令只能产出
/// "设备身份绑定握手"签名，绝不能被借去签其它协议内容（防签名混淆）。`context` 由
/// 握手层（P2）构造（双方临时 X25519 公钥 ‖ device_name ‖ username）。私钥不出 Rust。
#[tauri::command]
pub fn sign_device_identity(state: State<AppState>, context: Vec<u8>) -> Vec<u8> {
    const DOMAIN: &[u8] = b"ridge-id-bind-v1";
    let mut msg = Vec::with_capacity(DOMAIN.len() + context.len());
    msg.extend_from_slice(DOMAIN);
    msg.extend_from_slice(&context);
    state.device_identity.sign(&msg).to_vec()
}

/// 零信任 #1（概念 5）：校验 controller 经 CONTROL 通道发来的 **totp-bind** MAC。
/// host 用本机 TOTP 种子在 `transcript` 上 ±1 时间步窗口重算 tag 比对（恒定时间，见
/// `RemoteTotp::verify_bind_tag`）。`transcript` 由 host 握手层构造并传入（domain‖sorted
/// 双方临时公钥）；`tag` 为 controller 用当前 6 位码算的 HMAC。通过 → cloudHostBridge
/// 解门控、放行业务帧。与浏览器 `e2ee.ts::computeBindTag` 字节对齐（跨实现 golden 已锁）。
#[tauri::command]
pub fn verify_remote_totp_bind(state: State<AppState>, transcript: Vec<u8>, tag: Vec<u8>) -> bool {
    state.remote_auth.verify_bind_tag(&transcript, &tag)
}

/// §totp-persist：重置本机 TOTP 种子。重新生成 + 覆盖落盘（DPAPI/0600），已配对
/// 的 authenticator 立即失效，须重新扫码。发 `remote-totp-changed` 事件让面板刷新。
#[tauri::command]
pub fn remote_reset_totp(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.remote_auth.reset_totp();
    let _ = app.emit("remote-totp-changed", ());
    tracing::info!(target: "ridge::remote", "TOTP secret reset by user");
    Ok(())
}

/// §totp-persist：把活动 TOTP 种子切到指定云身份（`None`/登出 → `"default"`）。
/// 由前端在云登录态变化时调用，实现「不同账号不同种子」的实时切换。发
/// `remote-totp-changed` 事件让面板刷新二维码/验证码。
#[tauri::command]
pub fn remote_set_totp_identity(
    app: AppHandle,
    state: State<AppState>,
    username: Option<String>,
) -> Result<(), String> {
    state.remote_auth.switch_identity(username.as_deref());
    let _ = app.emit("remote-totp-changed", ());
    Ok(())
}

// ── TOTP 信任授权（grant_store，§totp-trust）──────────────────────────────────

/// §totp-trust-check：查询 `(当前身份, ctrl_pub_b64)` 是否持有 24h 内的信任授权。
///
/// `ctrl_pub_b64`：controller Ed25519 公钥的 base64 标准编码（32 字节）。
/// 返回 `true` → 跳过 TOTP 二次验证；`false` → 仍须手动 TOTP。
/// 解码失败视为「无授权」，返回 `false`（降级，不阻断远控）。
#[tauri::command]
pub fn totp_trust_check(state: State<AppState>, ctrl_pub_b64: &str) -> bool {
    let Ok(ctrl_pub) = base64::engine::general_purpose::STANDARD.decode(ctrl_pub_b64) else {
        tracing::warn!(target: "ridge::remote", "totp_trust_check: 解码 ctrl_pub_b64 失败，视为无授权");
        return false;
    };
    let identity = state.remote_auth.current_identity();
    ridge_core::grant_store::check(&identity, &ctrl_pub)
}

/// §totp-trust-record：记录/刷新 `(当前身份, ctrl_pub_b64)` 的信任时间戳为「当前时刻」。
///
/// 在手动 TOTP 验证通过后立即调用（前端决策）。幂等；写失败仅 warn。
#[tauri::command]
pub fn totp_trust_record(state: State<AppState>, ctrl_pub_b64: &str) -> Result<(), String> {
    let ctrl_pub = base64::engine::general_purpose::STANDARD
        .decode(ctrl_pub_b64)
        .map_err(|e| format!("ctrl_pub_b64 解码失败: {e}"))?;
    let identity = state.remote_auth.current_identity();
    ridge_core::grant_store::record(&identity, &ctrl_pub);
    Ok(())
}

/// §totp-trust-revoke-all：撤销当前身份的全部信任授权（删除对应 grants 文件）。
///
/// 用于：用户切换账号、主动「忘记所有受信控制端」、安全重置等场景。
#[tauri::command]
pub fn totp_trust_revoke_all(state: State<AppState>) {
    let identity = state.remote_auth.current_identity();
    ridge_core::grant_store::revoke_all(&identity);
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

/// §read-only: when enabled, remote `data-request` mutations (file writes,
/// deletes, git commit/push/…) are refused server-side. Reads stay allowed.
/// Defence-in-depth for view-only sessions — an authenticated remote already
/// has shell stdin, so this is a convenience guard, not an isolation boundary.
#[tauri::command]
pub fn set_remote_fs_readonly(state: State<AppState>, readonly: bool) -> Result<(), String> {
    state.remote_fs_readonly.store(readonly, Ordering::Relaxed);
    tracing::info!(target: "ridge::remote", readonly, "Remote filesystem read-only toggle changed");
    Ok(())
}

#[tauri::command]
pub fn get_remote_fs_readonly(state: State<AppState>) -> Result<bool, String> {
    Ok(state.remote_fs_readonly.load(Ordering::Relaxed))
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
