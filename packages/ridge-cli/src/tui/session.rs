//! 交互式 TUI 的会话抽象：把"输入回送 + 尺寸同步"与具体传输解耦。
//!
//! 本轮提供 [`LocalPtySession`]（本地 shell，passthrough 验证 + 自用终端）。
//! 后续（设计文档 §E4）将新增 `LanControllerSession`（连桌面 LAN host 的 WS 客户端）
//! 与 `CloudControllerSession`（公网 WebRTC offerer），二者实现同一 [`Session`] trait，
//! TUI 主循环 [`super::run_session`] 完全复用、无需改动。

use anyhow::Result;
use tokio::sync::mpsc;

use crate::pty::PtyBridge;

/// 一个可交互的远端/本地终端会话：回送输入、同步尺寸。
///
/// 输出方向不在 trait 里——会话创建时返回一个 `mpsc::Receiver<Vec<u8>>` 输出流，
/// 主循环把它原样透传到本地终端（passthrough）。
pub trait Session {
    /// 回送键盘/粘贴输入字节。
    fn send_input(&self, data: &[u8]) -> Result<()>;
    /// 同步终端尺寸（本地终端 resize / 初次对齐时调用）。
    fn resize(&self, cols: u16, rows: u16) -> Result<()>;
}

/// 本地 shell 会话：直接复用 [`PtyBridge`]。
pub struct LocalPtySession {
    bridge: PtyBridge,
}

impl LocalPtySession {
    /// 拉起本地 shell，返回会话 + 输出字节流。
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let (bridge, rx) = PtyBridge::spawn(shell, cwd)?;
        Ok((Self { bridge }, rx))
    }
}

impl Session for LocalPtySession {
    fn send_input(&self, data: &[u8]) -> Result<()> {
        self.bridge.write_input(data)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.bridge.resize(cols, rows)
    }
}
