//! 本地 shell 的 PTY 桥（契约 §9：行为对齐 `engine::pty`，但解耦 Tauri AppState）。
//!
//! TODO(reuse, 契约 §9/§11.D): `src-tauri/src/engine/pty.rs` 的 `spawn_pty_reader`
//! 与 `PtyHandle` 强依赖 Tauri `AppState`（窗口事件、workspaces map、teammate
//! 生命周期），无法在无头进程里复用；且 `mod engine;` 在 `src-tauri/src/lib.rs`
//! 里是私有，外部 crate 不可见。因此这里用**完全相同的 `portable-pty` 0.8 crate**
//! 直接拉起 shell，沿用上游的 8KiB 读缓冲 + 独立阻塞读线程的隔离思路（reader
//! panic 不影响主进程），但把输出送进一个 `tokio::mpsc` 通道而非 Tauri 事件。
//!
//! 若上游愿意把 PTY 读循环抽成与 AppState 无关的 `fn(reader, sink)` 并 `pub`，
//! 可直接复用（报告已列出该可选项）。

use std::io::{Read, Write};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::sync::Arc;
use tokio::sync::mpsc;

/// PTY 读缓冲大小（对齐上游 `engine::pty` 的 8192）。
const READ_BUF: usize = 8192;
/// 默认初始终端尺寸。
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// 已拉起的 PTY 句柄：可写入输入、可 resize；输出经 `output_rx` 流出。
pub struct PtyBridge {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyBridge {
    /// 拉起一个 shell。`shell` 为 None 时按平台选默认（Unix: $SHELL→bash→sh；
    /// Windows: pwsh→powershell→cmd）。返回桥句柄 + 输出字节流接收端。
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;

        let program = resolve_shell(shell);
        let mut cmd = CommandBuilder::new(&program);
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }
        // 让远端 vte 解析器拿到颜色 / 256 色能力。
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("failed to spawn shell '{program}'"))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
        spawn_reader_thread(reader, tx);

        Ok((
            Self {
                writer: Arc::new(Mutex::new(writer)),
                master: Arc::new(Mutex::new(pair.master)),
                _child: child,
            },
            rx,
        ))
    }

    /// 写入键盘 / 粘贴输入。
    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock();
        w.write_all(data).context("PTY write failed")?;
        w.flush().context("PTY flush failed")?;
        Ok(())
    }

    /// 调整终端尺寸。
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .lock()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("PTY resize failed")
    }
}

/// 选 shell 可执行名。
fn resolve_shell(shell: Option<&str>) -> String {
    if let Some(s) = shell {
        if !s.trim().is_empty() {
            return s.to_string();
        }
    }
    #[cfg(unix)]
    {
        if let Ok(sh) = std::env::var("SHELL") {
            if !sh.is_empty() {
                return sh;
            }
        }
        for cand in ["/bin/bash", "/usr/bin/bash", "/bin/zsh", "/bin/sh"] {
            if std::path::Path::new(cand).exists() {
                return cand.to_string();
            }
        }
        "/bin/sh".to_string()
    }
    #[cfg(windows)]
    {
        for cand in ["pwsh.exe", "powershell.exe", "cmd.exe"] {
            return cand.to_string();
        }
        "cmd.exe".to_string()
    }
}

/// 在独立**阻塞**线程里读 PTY，把字节块 `try_send` 进通道。对齐上游 `engine::pty`
/// 的隔离思路：读线程 panic 不波及主进程；EOF / 错误时关闭发送端。
fn spawn_reader_thread(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    std::thread::Builder::new()
        .name("ridge-cli-pty-reader".to_string())
        .spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF：子进程退出。
                    Ok(n) => {
                        // blocking_send 会在通道满时阻塞读线程（背压），避免丢字节。
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break; // 接收端已丢弃。
                        }
                    }
                    Err(e) => {
                        tracing::debug!(target: "ridge_cli::pty", error = %e, "PTY read error, reader exiting");
                        break;
                    }
                }
            }
        })
        .expect("failed to spawn PTY reader thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_shell_prefers_explicit() {
        assert_eq!(resolve_shell(Some("/bin/zsh")), "/bin/zsh");
        // 空串回退到默认探测。
        let def = resolve_shell(Some("  "));
        assert!(!def.is_empty());
    }
}
