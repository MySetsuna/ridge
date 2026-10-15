#[tauri::command]
pub fn get_remote_info(state: State<AppState>) -> Result<serde_json::Value, String> {
    let (port, enabled) = remote_transport_state(&state);
    let lan_ip = ridge_remote::net::detect_lan_ip();
    let lan_ips = ridge_remote::net::detect_lan_ips();
    let machine_name = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let auth: serde_json::Value = auth_request("remote_totp_info",serde_json::json!({"machineName":machine_name}))?;
    let totp_code = &auth["code"];
    let otpauth_uri = &auth["uri"];

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
                let sizes: Vec<serde_json::Value> = ws
                    .pane_sizes
                    .iter()
                    .map(|(id, (rows, cols))| {
                        serde_json::json!({
                            "pane": id.to_string(),
                            "rows": rows,
                            "cols": cols,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "ws": wid.to_string(),
                    "leaves": leaves.len(),
                    "terminals": ws.terminals.len(),
                    "pending": ws.pending_spawns.len(),
                    "orphanTerminals": orphan_terms,
                    "orphanPending": orphan_pend,
                    "sizes": sizes,
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

