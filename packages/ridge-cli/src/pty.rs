//! CLI shell projection of the kernel PTY registry.
//!
//! PTY spawn/write/resize/destroy semantics live in `ridge-kernel`; rdg owns
//! only terminal presentation and its lossless output receiver.

use std::sync::Arc;

use anyhow::Result;
use ridge_kernel::pty::PtyRegistry;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct PtyBridge {
    backend: PtyBackend,
}

enum PtyBackend {
    Local {
        registry: Arc<PtyRegistry>,
        id: Uuid,
    },
    /// A cloud session is a projection of the long-lived kernel PTY.  The
    /// session owns only an output lease; dropping it must never destroy the
    /// user's terminal when the WebRTC peer or desktop shell disconnects.
    Kernel {
        endpoint: ridge_kernel::registry::KernelEndpoint,
        id: Uuid,
        lease_id: Uuid,
    },
}

impl PtyBridge {
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        // Unit tests use an in-memory PTY so a machine-local kernel/registry
        // cannot consume their mock signaling timing. Production binaries
        // always take the kernel path; an explicit opt-in keeps kernel-backed
        // tests available when needed.
        let use_kernel = !cfg!(test) || std::env::var_os("RIDGE_TEST_USE_KERNEL").is_some();
        if use_kernel {
            let endpoint = ridge_kernel::client::running_endpoint().or_else(|| {
                crate::kernel_ctl::ensure_kernel_running().ok()
            });
            if let Some(endpoint) = endpoint {
                return Self::spawn_kernel(endpoint, shell, cwd);
            }
        }
        let registry = Arc::new(PtyRegistry::default());
        let (id, output) = registry.spawn_with_output(shell, cwd)?;
        Ok((Self { backend: PtyBackend::Local { registry, id } }, output))
    }

    fn spawn_kernel(
        endpoint: ridge_kernel::registry::KernelEndpoint,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        // Reattach the first live kernel PTY whenever possible.  The desktop
        // creates stable pane identities before this daemon starts, so this is
        // the normal path after a desktop restart or hard process kill.
        let info = ridge_kernel::client::list_domain_ptys(&endpoint)
            .map_err(|error| anyhow::anyhow!("list kernel PTYs: {error}"))?;
        let selected = cwd
            .filter(|cwd| !cwd.trim().is_empty())
            .and_then(|cwd| {
                info.iter().find(|pty| {
                    (pty.status == "running" || pty.status == "alive")
                        && pty.cwd.as_deref() == Some(cwd)
                })
            })
            .or_else(|| {
                info.iter()
                    .find(|pty| pty.status == "running" || pty.status == "alive")
            })
            .cloned();
        let existing_id = selected.as_ref().map(|pty| pty.pty_id);
        let id = existing_id.unwrap_or_else(Uuid::new_v4);
        if existing_id.is_none() {
            ridge_kernel::client::create_domain_pty(
                &endpoint,
                id,
                shell,
                cwd,
                None,
                "remote",
                Some("cloud-remote"),
            )
            .map_err(|error| anyhow::anyhow!("create kernel PTY: {error}"))?;
        }
        // Do not replay the entire bounded scrollback into every reconnecting
        // WebRTC session. The kernel remains the history owner; this lease
        // starts at the newest frame and only forwards output produced after
        // the detached host attached.
        let after_seq = selected.map(|pty| pty.next_seq.saturating_sub(1));
        let lease_id = ridge_kernel::client::attach_domain_pty_output(&endpoint, id, after_seq)
            .map_err(|error| anyhow::anyhow!("attach kernel PTY output: {error}"))?;
        let (tx, rx) = mpsc::channel(64);
        let poll_endpoint = endpoint.clone();
        tokio::spawn(async move {
            loop {
                let result = tokio::task::spawn_blocking({
                    let endpoint = poll_endpoint.clone();
                    move || ridge_kernel::client::poll_domain_pty_output(&endpoint, id, lease_id, 1000, 64)
                })
                .await;
                match result {
                    Ok(Ok(ridge_kernel::client::KernelPtyOutput::Data(bytes))) if !bytes.is_empty() => {
                        if tx.send(bytes).await.is_err() { break; }
                    }
                    Ok(Ok(ridge_kernel::client::KernelPtyOutput::Timeout)) => {}
                    Ok(Ok(ridge_kernel::client::KernelPtyOutput::Lagged)) => {
                        let _ = ridge_kernel::client::resync_domain_pty_output(&poll_endpoint, id, lease_id);
                    }
                    Ok(Err(error)) => {
                        tracing::debug!(target: "ridge_cli::pty", %error, "kernel output lease ended");
                        break;
                    }
                    Err(error) => {
                        tracing::debug!(target: "ridge_cli::pty", %error, "kernel output poll task ended");
                        break;
                    }
                    _ => {}
                }
            }
        });
        Ok((Self { backend: PtyBackend::Kernel { endpoint, id, lease_id } }, rx))
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        match &self.backend {
            PtyBackend::Local { registry, id } => registry.write(*id, data),
            PtyBackend::Kernel { endpoint, id, .. } => ridge_kernel::client::write_domain_pty(endpoint, *id, data)
                .map_err(|error| anyhow::anyhow!(error)),
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        match &self.backend {
            PtyBackend::Local { registry, id } => registry.resize(*id, cols, rows),
            PtyBackend::Kernel { endpoint, id, .. } => ridge_kernel::client::resize_domain_pty(endpoint, *id, cols, rows)
                .map_err(|error| anyhow::anyhow!(error)),
        }
    }
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        if let PtyBackend::Kernel { endpoint, id, lease_id } = &self.backend {
            let _ = ridge_kernel::client::detach_domain_pty_output(endpoint, *id, *lease_id);
        } else if let PtyBackend::Local { registry, id } = &self.backend {
            let _ = registry.destroy(*id);
        }
    }
}
