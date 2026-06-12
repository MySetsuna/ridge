//! 会话控制帧 + host→controller 业务响应载荷（E2EE 明文，mux 之内）。
//!
//! 统一远控 S3（契约 §11.1）后，cli host 的 controller↔host 线协议**收敛到桌面同款**：
//!   - 业务面（终端输入/resize、搜索、文件树）走 [`crate::rpc`] 的 JSON-RPC 2.0（0x11）。
//!   - PTY 输出走 [`crate::mux`] 的 PANE_RAW（0x10）。
//!   - 本文件只保留**会话控制帧**（契约 §4 TOTP 握手，0x12 通道）的线形类型。
//!
//! 旧的裸 `ControlMsg` / `HostMsg`（无通道字节的 controller→host JSON + 0x10/0x11
//! 输出）已被 mux + JSON-RPC 取代——那套只有 terminal/wasm-vte controller 能讲，
//! 没有浏览器 controller 对端（§11.1 根因），故移除。

use serde::{Deserialize, Serialize};

/// 0x12 CONTROL 通道帧（契约 §4 + 零信任 #1）。`t` tag + kebab-case，与桌面
/// `cloudHostBridge.ts` / 浏览器 controller 逐字段一致：
///   - controller → host: `{"t":"totp-verify","code":"123456"}`（明文码，旧/回落路径）
///   - controller → host: `{"t":"totp-bind","tag":"<base64 HMAC>"}`（信道绑定，码不上线）
///   - host → controller: `{"t":"totp-result","ok":true}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum SessionControl {
    /// controller 把用户从 host TUI 读到的 6 位 TOTP 回传。
    TotpVerify { code: String },
    /// 零信任 #1：controller 在收到 host 0x02 后改发本会话 transcript 上的 HMAC tag
    /// （base64），明文 6 位码**不上线**。host 用本机种子 ±1 窗口重算比对。
    TotpBind { tag: String },
    /// host 回二次验证结果。
    TotpResult { ok: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约 §4：controller→host 的 totp-verify 必须解析成
    /// `{"t":"totp-verify","code":"…"}`。锁死跨实现对齐的 tag/字段名。
    #[test]
    fn totp_verify_parses_contract_shape() {
        let m: SessionControl =
            serde_json::from_str(r#"{"t":"totp-verify","code":"123456"}"#).unwrap();
        assert_eq!(
            m,
            SessionControl::TotpVerify {
                code: "123456".into()
            }
        );
    }

    /// 契约 §4：host→controller 的 totp-result 必须序列化成
    /// `{"t":"totp-result","ok":true}`（kebab-case 的 `t` tag）。
    #[test]
    fn totp_result_serializes_contract_shape() {
        let json = serde_json::to_string(&SessionControl::TotpResult { ok: true }).unwrap();
        assert!(json.contains("\"t\":\"totp-result\""), "got: {json}");
        assert!(json.contains("\"ok\":true"), "got: {json}");
    }

    #[test]
    fn totp_result_roundtrips() {
        let back: SessionControl =
            serde_json::from_str(r#"{"t":"totp-result","ok":false}"#).unwrap();
        assert_eq!(back, SessionControl::TotpResult { ok: false });
    }
}
