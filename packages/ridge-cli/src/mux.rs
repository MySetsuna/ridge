//! 1 字节通道前缀的 mux 编解码（统一远控 S3 / 契约 §7 / §11.1）。
//!
//! E2EE 明文帧的首字节标识其逻辑通道，与桌面端 `src/lib/transport/remote/cloudMux.ts`
//! **逐字节一致**——这是收敛的核心：同一个浏览器 controller 既能驱动桌面 host，
//! 也能驱动本 cli host。
//!
//!   0x10  PANE_RAW  — 某个 pane 的裸 PTY 字节（高频、单向 host→controller）。
//!                     线形：`0x10 || paneIdLen(u8) || paneId(UTF-8, ≤255) || raw`。
//!   0x11  JSON      — UTF-8 JSON：JSON-RPC 2.0 业务信封（控制 / 事件 / invoke）。
//!   0x12  CONTROL   — UTF-8 JSON：会话控制帧（契约 §4 TOTP 握手），与 0x11 分离，
//!                     以便 host 在门控业务帧的同时仍处理 TOTP。
//!
//! 本模块是纯函数（无 I/O、无 E2EE），可独立单测，收发两向复用。
//!
//! 与旧 `protocol.rs` 的差异（收敛动机，§11.1）：旧版 host→controller 只用 0x10/0x11，
//! 且 0x10 **只**裸字节无 paneId（单 pane 隐式），controller→host 是无通道字节的裸
//! `ControlMsg` JSON。桌面 controller（JSON-RPC + mux）讲不通这套。新版让 cli 在两个
//! 方向都讲桌面同款 mux + JSON-RPC，故同一 controller 可同时驱动两类 host。

use serde::Serialize;

/// 逻辑通道标识（与 `cloudMux.ts` 的 `CHANNEL` 逐字节一致）。
pub mod channel {
    /// 某个 pane 的裸 PTY 字节。
    pub const PANE_RAW: u8 = 0x10;
    /// UTF-8 JSON（JSON-RPC 2.0 业务信封）。
    pub const JSON: u8 = 0x11;
    /// UTF-8 JSON 会话控制帧（契约 §4 TOTP 握手）。
    pub const CONTROL: u8 = 0x12;
}

/// paneId 在线上可占的最大字节数（1 字节长度前缀）。与 `cloudMux.ts` 的
/// `MAX_PANE_ID_BYTES` 一致。
pub const MAX_PANE_ID_BYTES: usize = 255;

/// demux 后的一帧入站帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inbound {
    /// 0x11 JSON-RPC 业务信封（未解析的 UTF-8 JSON 字节）。
    Json(Vec<u8>),
    /// 0x12 会话控制帧（未解析的 UTF-8 JSON 字节）。
    Control(Vec<u8>),
    /// 0x10 pane 字节（controller 一般不发；保留以备前向兼容）。
    Pane { pane_id: String, bytes: Vec<u8> },
    /// 未知通道 tag（前向兼容：忽略）。
    Unknown(u8),
    /// 空帧。
    Empty,
}

/// 编码一帧 0x11 JSON 业务信封：`0x11 || utf8(JSON)`。
pub fn encode_json<T: Serialize>(value: &T) -> Vec<u8> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(channel::JSON);
    out.extend_from_slice(&body);
    out
}

/// 编码一帧 0x12 会话控制帧：`0x12 || utf8(JSON)`（契约 §4）。
pub fn encode_control<T: Serialize>(value: &T) -> Vec<u8> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(channel::CONTROL);
    out.extend_from_slice(&body);
    out
}

/// 编码一帧 pane 字节：`0x10 || paneIdLen(1) || paneId(UTF-8) || raw`。
/// paneId 的 UTF-8 字节数超过 [`MAX_PANE_ID_BYTES`] 时返回 `Err`（调用方丢弃该帧，不断连）。
pub fn encode_pane(pane_id: &str, bytes: &[u8]) -> Result<Vec<u8>, PaneFrameError> {
    let id_bytes = pane_id.as_bytes();
    if id_bytes.len() > MAX_PANE_ID_BYTES {
        return Err(PaneFrameError::PaneIdTooLong(id_bytes.len()));
    }
    let mut out = Vec::with_capacity(1 + 1 + id_bytes.len() + bytes.len());
    out.push(channel::PANE_RAW);
    out.push(id_bytes.len() as u8);
    out.extend_from_slice(id_bytes);
    out.extend_from_slice(bytes);
    Ok(out)
}

/// pane 帧编码错误（paneId 过长）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFrameError {
    /// paneId 的 UTF-8 字节数超过上限。
    PaneIdTooLong(usize),
}

impl std::fmt::Display for PaneFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaneFrameError::PaneIdTooLong(n) => {
                write!(f, "paneId too long ({n} > {MAX_PANE_ID_BYTES} bytes)")
            }
        }
    }
}

impl std::error::Error for PaneFrameError {}

/// 按首字节通道标识 demux 一帧入站 E2EE 明文。短帧/未知 tag 不报错（返回
/// `Unknown`/`Empty`），由调用方决定如何处理坏帧（与 `cloudMux.ts` 的「记录+丢弃」
/// 立场一致）。JSON 解析延后到调用方，本函数只切出字节。
pub fn demux(frame: &[u8]) -> Inbound {
    let Some(&tag) = frame.first() else {
        return Inbound::Empty;
    };
    match tag {
        channel::JSON => Inbound::Json(frame[1..].to_vec()),
        channel::CONTROL => Inbound::Control(frame[1..].to_vec()),
        channel::PANE_RAW => {
            // 至少需要 tag + 长度字节。
            if frame.len() < 2 {
                return Inbound::Unknown(tag);
            }
            let id_len = frame[1] as usize;
            let id_end = 2 + id_len;
            if frame.len() < id_end {
                return Inbound::Unknown(tag);
            }
            let pane_id = String::from_utf8_lossy(&frame[2..id_end]).into_owned();
            let bytes = frame[id_end..].to_vec();
            Inbound::Pane { pane_id, bytes }
        }
        other => Inbound::Unknown(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_frame_roundtrips_through_demux() {
        let framed = encode_json(&json!({"jsonrpc":"2.0","id":1,"method":"x"}));
        assert_eq!(framed[0], channel::JSON);
        match demux(&framed) {
            Inbound::Json(body) => {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(v["method"], "x");
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn control_frame_roundtrips_through_demux() {
        let framed = encode_control(&json!({"t":"totp-result","ok":true}));
        assert_eq!(framed[0], channel::CONTROL);
        match demux(&framed) {
            Inbound::Control(body) => {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(v["t"], "totp-result");
                assert_eq!(v["ok"], true);
            }
            other => panic!("expected Control, got {other:?}"),
        }
    }

    #[test]
    fn pane_frame_layout_matches_cloudmux() {
        // 0x10 || len || id || raw — 与 cloudMux.ts encodePaneFrame 逐字节对齐。
        let framed = encode_pane("pane-7", b"hello").unwrap();
        assert_eq!(framed[0], channel::PANE_RAW);
        assert_eq!(framed[1] as usize, "pane-7".len());
        assert_eq!(&framed[2..2 + 6], b"pane-7");
        assert_eq!(&framed[2 + 6..], b"hello");
    }

    #[test]
    fn pane_frame_demuxes_back() {
        let framed = encode_pane("abc", b"\x1b[0m").unwrap();
        match demux(&framed) {
            Inbound::Pane { pane_id, bytes } => {
                assert_eq!(pane_id, "abc");
                assert_eq!(bytes, b"\x1b[0m");
            }
            other => panic!("expected Pane, got {other:?}"),
        }
    }

    #[test]
    fn pane_id_too_long_is_rejected() {
        let long = "x".repeat(MAX_PANE_ID_BYTES + 1);
        assert!(matches!(
            encode_pane(&long, b""),
            Err(PaneFrameError::PaneIdTooLong(_))
        ));
    }

    #[test]
    fn empty_frame_is_empty() {
        assert_eq!(demux(&[]), Inbound::Empty);
    }

    #[test]
    fn unknown_tag_is_unknown() {
        assert_eq!(demux(&[0x99, 1, 2, 3]), Inbound::Unknown(0x99));
    }

    #[test]
    fn short_pane_frame_is_unknown() {
        // 0x10 但缺长度字节 → Unknown（不 panic）。
        assert_eq!(
            demux(&[channel::PANE_RAW]),
            Inbound::Unknown(channel::PANE_RAW)
        );
        // 长度字节声称 5 字节 id 但实际不足 → Unknown。
        assert_eq!(
            demux(&[channel::PANE_RAW, 5, b'a']),
            Inbound::Unknown(channel::PANE_RAW)
        );
    }
}
