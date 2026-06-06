//! 桌面 LAN host 远控线协议的**纯编解码层**（E4 地基）。
//!
//! 与桌面自带的浏览器客户端 `src/remote/lib/wsRemote.ts` **逐字节同契约**：
//! - 入站二进制帧：16 字节 paneId（UUID 原始字节）+ 其后为该 pane 的 PTY 原始字节。
//! - 出站为 JSON 文本：`{"type":"subscribe-pane"|"stdin"|"claim-pane"|"list-panes"|"ping",...}`。
//! - WS 端点：`wss://host:port/ws?code=<TOTP>`（或 `?token=<session>`）。
//!
//! 这一层是纯函数 + 可单测，把"协议正确性"与"网络/TLS 接线"解耦。E4 的 WS 驱动
//! （tokio-tungstenite + 自签 TLS 接受）只需调用这里的编解码，再把入站 PTY 字节灌进
//! `super::run_session` 的输出通道、把按键经 [`stdin`] 回送即可。
//!
//! 注：`#[allow(dead_code)]` —— 编解码已就绪并自测，WS 驱动落地后即被引用。
#![allow(dead_code)]

use serde_json::json;

/// 入站二进制帧（某 pane 的一段 PTY 输出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundFrame {
    pub pane_id: String,
    pub bytes: Vec<u8>,
}

/// 解析入站二进制帧：前 16 字节为 UUID（连字符化），其后为 PTY 字节。
/// 长度不足 16 返回 `None`（非法帧，忽略）。
pub fn parse_binary_frame(buf: &[u8]) -> Option<InboundFrame> {
    if buf.len() < 16 {
        return None;
    }
    Some(InboundFrame {
        pane_id: uuid_from_bytes(&buf[..16]),
        bytes: buf[16..].to_vec(),
    })
}

/// 16 字节 → `8-4-4-4-12` 小写十六进制 UUID（对齐 wsRemote.ts 的 `uuidFromBytes`）。
fn uuid_from_bytes(b: &[u8]) -> String {
    debug_assert_eq!(b.len(), 16);
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// `{"type":"list-panes"}`。
pub fn list_panes() -> String {
    json!({ "type": "list-panes" }).to_string()
}

/// `{"type":"subscribe-pane","paneId":<id>}`。
pub fn subscribe_pane(pane_id: &str) -> String {
    json!({ "type": "subscribe-pane", "paneId": pane_id }).to_string()
}

/// `{"type":"create-pane"}`。host 在本端 active workspace 新建并激活一个终端，
/// 回 `{"type":"create-pane-result","success":true,"paneId":<id>}`。用于首次连接
/// 时 `list-panes` 返回空（无终端）的场景，验证经 CDP 联调确认（见
/// scripts/cdp-lan-probe.mjs）。
pub fn create_pane() -> String {
    json!({ "type": "create-pane" }).to_string()
}

/// `{"type":"stdin","paneId":<id>,"data":<utf8>}`。键盘/粘贴回送。
pub fn stdin(pane_id: &str, data: &str) -> String {
    json!({ "type": "stdin", "paneId": pane_id, "data": data }).to_string()
}

/// `{"type":"claim-pane",...,"seq":<n>}`。按本端视口尺寸 reflow 远端真实 PTY
/// （`resize` 仅记账不 reflow，故 TUI resize 走 claim-pane，与 wsRemote 一致）。
pub fn claim_pane(pane_id: &str, rows: u16, cols: u16, seq: u64) -> String {
    json!({
        "type": "claim-pane",
        "paneId": pane_id,
        "rows": rows,
        "cols": cols,
        "pixelWidth": 0,
        "pixelHeight": 0,
        "seq": seq,
    })
    .to_string()
}

/// `{"type":"ping"}`（心跳；host 回 `{"type":"pong"}`）。
pub fn ping() -> String {
    json!({ "type": "ping" }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parses_binary_frame() {
        // 16 字节 paneId（0x00..0x0f）+ 负载 "hi"。
        let mut buf: Vec<u8> = (0u8..16).collect();
        buf.extend_from_slice(b"hi");
        let f = parse_binary_frame(&buf).expect("valid frame");
        assert_eq!(f.pane_id, "00010203-0405-0607-0809-0a0b0c0d0e0f");
        assert_eq!(f.bytes, b"hi");
    }

    #[test]
    fn rejects_short_frame() {
        assert!(parse_binary_frame(&[0u8; 15]).is_none());
    }

    #[test]
    fn empty_payload_is_valid() {
        let buf = vec![0xabu8; 16];
        let f = parse_binary_frame(&buf).expect("valid");
        assert_eq!(f.pane_id, "abababab-abab-abab-abab-abababababab");
        // 32 个十六进制字符 + 4 个连字符 = 36。
        assert_eq!(f.pane_id.len(), 36);
        assert!(f.bytes.is_empty());
    }

    #[test]
    fn encoders_shape() {
        let v: Value = serde_json::from_str(&subscribe_pane("p1")).unwrap();
        assert_eq!(v["type"], "subscribe-pane");
        assert_eq!(v["paneId"], "p1");

        let v: Value = serde_json::from_str(&stdin("p1", "ls\r")).unwrap();
        assert_eq!(v["type"], "stdin");
        assert_eq!(v["data"], "ls\r");

        let v: Value = serde_json::from_str(&claim_pane("p1", 24, 80, 3)).unwrap();
        assert_eq!(v["type"], "claim-pane");
        assert_eq!(v["rows"], 24);
        assert_eq!(v["cols"], 80);
        assert_eq!(v["seq"], 3);

        assert_eq!(serde_json::from_str::<Value>(&ping()).unwrap()["type"], "ping");
        assert_eq!(serde_json::from_str::<Value>(&list_panes()).unwrap()["type"], "list-panes");
    }
}
