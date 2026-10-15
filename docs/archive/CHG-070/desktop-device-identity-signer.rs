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

