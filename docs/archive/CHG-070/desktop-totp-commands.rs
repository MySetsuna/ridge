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
pub fn get_device_identity_pub() -> Result<Vec<u8>, String> {
    super::runtime::request("get_device_identity_pub", serde_json::json!({}), true, super::runtime::window_connection("device-identity"))
}

/// Domain-separated device binding signature; private key remains in Runtime.
#[tauri::command]
pub fn sign_device_identity(context: Vec<u8>) -> Result<Vec<u8>, String> {
    super::runtime::request("sign_device_identity", serde_json::json!({"context":context}), true, super::runtime::window_connection("device-identity"))
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

