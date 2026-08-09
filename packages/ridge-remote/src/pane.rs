//! `ridge_remote::pane` —— 远控 pane 字节流的**帧格式 + 重同步策略 SSOT**。
//!
//! 三条 pane 服务腿曾各自手抄这份（源码注释自认「逐字一致」「同名同值」）：
//! - 桌面 LAN：`src-tauri/src/remote_host_impl.rs::handle_ws`
//! - 桌面 cloud：`src-tauri/src/commands/cloud_pane.rs`
//! - rdg LAN：`packages/ridge-cli/src/tui/lan_host_impl.rs::run_ws`
//!
//! 收口为一份：各腿的 I/O 管道（WS `Message::Binary` / Tauri event `pane-raw-*` /
//! `broadcast`，差异真实不可消）保留，但**只调这里的帧构造 + 常量**，令协议改动
//! 一处生效、不再漂移。帧体（`RIS + 模式前导 + scrollback`）本就是
//! [`ridge_term::term::modes::build_resync_frame`] 一份 SSOT。

use std::time::Duration;

use ridge_term::term::modes::{build_resync_frame, Modes};
use uuid::Uuid;

/// 重同步限频：≥1s 一次，防「慢消费 → 丢帧 → 重同步 → 更慢」的拥塞放大反馈环。
pub const RESYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// 每 pane 转发通道容量。满即丢帧（控制端 vte 因空洞失同步 → desync→resync 自愈）。
pub const RAW_CHAN_CAP: usize = 512;

/// LAN（WebSocket，无分片层）重同步回放 scrollback 上限：64 KiB。
pub const RESYNC_SCROLLBACK_LAN: usize = 65536;

/// Cloud（经分片层 ≤16 KiB/帧，可安全发大块）重同步回放上限：256 KiB，
/// 覆盖深历史终端（长构建输出、vim 会话），减少初次连接的历史缺失。
pub const RESYNC_SCROLLBACK_CLOUD: usize = 262144;

/// pane-id 二进制前缀长度（控制端 `wsRemote.ts` 从 offset 0 读 16 字节 UUID）。
pub const PANE_ID_PREFIX_LEN: usize = 16;

/// 一帧 live pane 字节：16 字节 pane-id 前缀 + 原始 PTY 载荷。
/// LAN WebSocket 腿用之（cloud 腿以 Tauri 事件名携 pane-id，无需前缀）。
pub fn pane_frame(pane_id: Uuid, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PANE_ID_PREFIX_LEN + payload.len());
    out.extend_from_slice(pane_id.as_bytes());
    out.extend_from_slice(payload);
    out
}

/// 一帧 pane 重同步：16B pane-id 前缀 + `RIS + 模式前导 + scrollback`
/// （见 [`build_resync_frame`]）。新连/重连/背压自愈时发，令控制端镜像内核重建
/// 鼠标上报 / alt 屏等一次性开启态（早滑出 scrollback 尾，否则控制端鼠标失灵）。
pub fn pane_resync_frame(
    pane_id: Uuid,
    scrollback: &[u8],
    modes: &Modes,
    alt_screen: bool,
) -> Vec<u8> {
    let body = build_resync_frame(scrollback, modes, alt_screen);
    pane_frame(pane_id, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_frame_prefixes_16_byte_uuid() {
        let id = Uuid::from_u128(0x0123456789abcdef0123456789abcdef);
        let f = pane_frame(id, b"payload");
        assert_eq!(&f[..16], id.as_bytes());
        assert_eq!(&f[16..], b"payload");
    }

    #[test]
    fn pane_frame_empty_payload_is_just_prefix() {
        let id = Uuid::from_u128(1);
        let f = pane_frame(id, b"");
        assert_eq!(f.len(), 16);
        assert_eq!(&f[..], id.as_bytes());
    }

    #[test]
    fn resync_frame_carries_prefix_then_ris_then_mode_reattach() {
        let id = Uuid::from_u128(2);
        // Active button-event mouse + SGR + alt screen — the TUI case whose
        // one-time enables must be reasserted for the controller mirror.
        let mut modes = Modes::default();
        modes.mouse_button_event = true;
        modes.mouse_sgr = true;
        let f = pane_resync_frame(id, b"scrollback-tail", &modes, true);
        assert_eq!(&f[..16], id.as_bytes(), "16B pane-id prefix first");
        let body = &f[16..];
        assert!(body.starts_with(b"\x1bc"), "RIS resets the mirror first");
        // window() search for the reattach escapes the preamble must contain.
        let contains = |needle: &[u8]| body.windows(needle.len()).any(|w| w == needle);
        assert!(contains(b"\x1b[?1049h"), "alt screen reattach");
        assert!(contains(b"\x1b[?1002h"), "button-event mouse reattach");
        assert!(contains(b"\x1b[?1006h"), "SGR mouse-encoding reattach");
        assert!(
            body.ends_with(b"scrollback-tail"),
            "scrollback rides at the tail"
        );
    }

    #[test]
    fn resync_frame_plain_shell_has_no_mode_escapes_only_ris_and_scrollback() {
        let id = Uuid::from_u128(3);
        let f = pane_resync_frame(id, b"$ ", &Modes::default(), false);
        let body = &f[16..];
        assert_eq!(
            body, b"\x1bc$ ",
            "RIS + scrollback, empty preamble for a plain shell"
        );
    }
}
