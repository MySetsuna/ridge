//! 内层业务协议（E2EE 明文载荷）。
//!
//! 契约 §7 规定内层明文 = 现有 `postcard` 二进制增量协议帧，但那套 schema 归
//! 桌面端 / ridge-term 所有（`packages/ridge-term`），且 controller 跑自己的 wasm
//! vte 解析器消费**原始 PTY 字节**（参见 src-tauri/src/lib.rs 的
//! `RemotePtyEvent::RawBytes` 路径——桌面端正是把裸字节转发给远端，远端
//! `kernel.feed()` 自行解析）。
//!
//! 因此无头 host 侧采用同样的“裸字节转发”模型：
//! - host→controller：PTY 输出原始字节（经 §7 攒批 + E2EE）。
//! - controller→host：控制消息（键盘输入、resize、文件搜索 / 文件树请求）。
//!
//! 这里用一个最小 JSON tagged 协议描述 controller→host 的控制面，host→controller
//! 的 PTY 字节走二进制（前缀 1 字节通道标识）。两者都在 E2EE 之内。

use serde::{Deserialize, Serialize};

/// host→controller 二进制帧的首字节通道标识。controller 据此区分裸 PTY 字节
/// 与（未来的）带外消息。
pub mod channel {
    /// 后续字节是 PTY 原始输出。
    pub const PTY_OUTPUT: u8 = 0x10;
    /// 后续字节是 UTF-8 JSON（host→controller 的带外响应，如搜索结果）。
    pub const JSON: u8 = 0x11;
}

/// controller→host 的控制消息（JSON 明文，在 E2EE 之内）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ControlMsg {
    /// 键盘 / 粘贴输入，写入 PTY。
    Input { data: String },
    /// 终端尺寸变化。
    Resize { cols: u16, rows: u16 },
    /// ripgrep 级文本搜索（契约 §9 复用 fs::search）。
    Search {
        root: String,
        query: String,
        #[serde(default)]
        use_regex: bool,
        #[serde(default)]
        case_sensitive: bool,
    },
    /// 列目录（契约 §9 复用 fs::tree）。
    Tree { path: String },
}

/// host→controller 的带外响应（JSON，channel::JSON 之后）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum HostMsg {
    /// 搜索结果。
    SearchResult {
        results: Vec<crate::fs_reuse::SearchResult>,
    },
    /// 目录列表。
    Tree {
        entries: Vec<crate::fs_reuse::FileNode>,
    },
    /// 错误（人类可读，不泄露内部路径细节）。
    Error { message: String },
}

/// 给一段 PTY 输出加上通道前缀。
pub fn frame_pty_output(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + bytes.len());
    out.push(channel::PTY_OUTPUT);
    out.extend_from_slice(bytes);
    out
}

/// 给一条 host JSON 消息加上通道前缀。
pub fn frame_host_json(msg: &HostMsg) -> Vec<u8> {
    let json = serde_json::to_vec(msg).unwrap_or_default();
    let mut out = Vec::with_capacity(1 + json.len());
    out.push(channel::JSON);
    out.extend_from_slice(&json);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_msg_roundtrip() {
        let m = ControlMsg::Resize {
            cols: 120,
            rows: 40,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"t\":\"resize\""));
        let back: ControlMsg = serde_json::from_str(&s).unwrap();
        matches!(
            back,
            ControlMsg::Resize {
                cols: 120,
                rows: 40
            }
        );
    }

    #[test]
    fn input_msg_parses() {
        let m: ControlMsg = serde_json::from_str(r#"{"t":"input","data":"ls\n"}"#).unwrap();
        match m {
            ControlMsg::Input { data } => assert_eq!(data, "ls\n"),
            _ => panic!("expected input"),
        }
    }

    #[test]
    fn pty_output_framing_prefixes_channel() {
        let f = frame_pty_output(b"abc");
        assert_eq!(f[0], channel::PTY_OUTPUT);
        assert_eq!(&f[1..], b"abc");
    }
}
