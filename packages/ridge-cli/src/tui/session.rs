//! 交互式 TUI 的会话抽象：把输入、尺寸同步与具体传输解耦。
//!
//! TUI shell 会话只连接长期运行的 `ridge-kernel` PTY。输出通过可取消的
//! output lease 读取；外壳退出不会销毁 Kernel 子进程，后续可按稳定 pane
//! UUID 重新接入。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;

use anyhow::Result;
use ridge_kernel::client::{
    attach_domain_pty_output, create_domain_pty, destroy_domain_pty, detach_domain_pty_output,
    list_domain_ptys, poll_domain_pty_output, resize_domain_pty, resync_domain_pty_output,
    write_domain_pty, KernelPtyOutput,
};
use ridge_kernel::registry::KernelEndpoint;
use tokio::sync::mpsc;
use uuid::Uuid;

#[cfg(test)]
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

/// TUI shell 会话：只持有外部 `ridge-kernel` 的稳定 PTY 引用。
pub struct LocalPtySession {
    backend: SessionBackend,
}

enum SessionBackend {
    Kernel {
        endpoint: KernelEndpoint,
        id: Uuid,
        stop: Arc<AtomicBool>,
    },
    #[cfg(test)]
    Local(PtyBridge),
}

impl LocalPtySession {
    /// 创建或复接外部 Kernel PTY，返回会话 + 输出字节流。
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        Self::spawn_with_id(Uuid::new_v4(), shell, cwd)
    }

    /// 使用调用方拥有的稳定 pane UUID 创建或复接 PTY。
    pub fn spawn_with_id(
        id: Uuid,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let endpoint = match crate::kernel_ctl::ensure_kernel_running() {
            Ok(endpoint) => endpoint,
            Err(error) => {
                #[cfg(test)]
                {
                    let _ = &error;
                    let (bridge, rx) = PtyBridge::spawn(shell, cwd)?;
                    return Ok((
                        Self {
                            backend: SessionBackend::Local(bridge),
                        },
                        rx,
                    ));
                }
                #[cfg(not(test))]
                return Err(anyhow::Error::msg(error));
            }
        };
        let existing = list_domain_ptys(&endpoint)
            .map_err(anyhow::Error::msg)?
            .into_iter()
            .find(|info| info.pty_id == id || info.id == id);
        let created = existing.is_none();
        let (pty_id, after_seq) = if let Some(info) = existing {
            (info.pty_id, Some(info.next_seq.saturating_sub(1)))
        } else {
            let pty_id = create_domain_pty(
                &endpoint,
                id,
                shell,
                cwd,
                None,
                "shell",
                Some("ridge-interactive"),
            )
            .map_err(anyhow::Error::msg)?;
            (pty_id, None)
        };
        let lease_id = match attach_domain_pty_output(&endpoint, pty_id, after_seq) {
            Ok(lease_id) => lease_id,
            Err(error) => {
                if created {
                    let _ = destroy_domain_pty(&endpoint, pty_id);
                }
                return Err(anyhow::Error::msg(error));
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel(256);
        spawn_output_pump(endpoint.clone(), pty_id, lease_id, stop.clone(), tx);
        Ok((
            Self {
                backend: SessionBackend::Kernel {
                    endpoint,
                    id: pty_id,
                    stop,
                },
            },
            rx,
        ))
    }
}

impl Session for LocalPtySession {
    fn send_input(&self, data: &[u8]) -> Result<()> {
        match &self.backend {
            SessionBackend::Kernel { endpoint, id, .. } => {
                write_domain_pty(endpoint, *id, data).map_err(anyhow::Error::msg)
            }
            #[cfg(test)]
            SessionBackend::Local(bridge) => bridge.write_input(data),
        }
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        match &self.backend {
            SessionBackend::Kernel { endpoint, id, .. } => {
                resize_domain_pty(endpoint, *id, cols, rows).map_err(anyhow::Error::msg)
            }
            #[cfg(test)]
            SessionBackend::Local(bridge) => bridge.resize(cols, rows),
        }
    }
}

impl Drop for LocalPtySession {
    fn drop(&mut self) {
        #[cfg(not(test))]
        let SessionBackend::Kernel { stop, .. } = &self.backend;
        #[cfg(test)]
        let Some(stop) = (match &self.backend {
            SessionBackend::Kernel { stop, .. } => Some(stop),
            SessionBackend::Local(_) => None,
        }) else {
            return;
        };
        // Kernel owns the child process. Stop only this shell's output lease;
        // the PTY remains available for a later pane reattach.
        stop.store(true, Ordering::Release);
    }
}

fn spawn_output_pump(
    endpoint: KernelEndpoint,
    pty_id: Uuid,
    lease_id: Uuid,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Vec<u8>>,
) {
    thread::spawn(move || {
        while !stop.load(Ordering::Acquire) && poll_output_once(&endpoint, pty_id, lease_id, &tx) {}
        let _ = detach_domain_pty_output(&endpoint, pty_id, lease_id);
    });
}

fn poll_output_once(
    endpoint: &KernelEndpoint,
    pty_id: Uuid,
    lease_id: Uuid,
    tx: &mpsc::Sender<Vec<u8>>,
) -> bool {
    match poll_domain_pty_output(endpoint, pty_id, lease_id, 500, 64) {
        Ok(KernelPtyOutput::Data(bytes)) if !bytes.is_empty() => tx.blocking_send(bytes).is_ok(),
        Ok(KernelPtyOutput::Data(_)) | Ok(KernelPtyOutput::Timeout) => true,
        Ok(KernelPtyOutput::Lagged) => resync_domain_pty_output(endpoint, pty_id, lease_id).is_ok(),
        Err(_) => false,
    }
}
