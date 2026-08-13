//! CLI shell projection of the kernel PTY registry.
//!
//! PTY spawn/write/resize/destroy semantics live in `ridge-kernel`; rdg owns
//! only terminal presentation and its lossless output receiver.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use ridge_kernel::pty::PtyRegistry;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

const REMOTE_PTY_BINDING_SCHEMA: u32 = 1;
const REMOTE_PTY_BINDING_FILE_ENV: &str = "RIDGE_REMOTE_PTY_BINDING_FILE";
const REMOTE_PTY_PROFILE: &str = "cloud-remote";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RemotePtyBinding {
    schema: u32,
    pty_id: Uuid,
    workspace_id: Option<Uuid>,
    cwd: Option<String>,
}

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
            let endpoint = ridge_kernel::client::running_endpoint()
                .or_else(|| crate::kernel_ctl::ensure_kernel_running().ok());
            if let Some(endpoint) = endpoint {
                return Self::spawn_kernel(endpoint, shell, cwd);
            }
        }
        let registry = Arc::new(PtyRegistry::default());
        let (id, output) = registry.spawn_with_output(shell, cwd)?;
        Ok((
            Self {
                backend: PtyBackend::Local { registry, id },
            },
            output,
        ))
    }

    fn spawn_kernel(
        endpoint: ridge_kernel::registry::KernelEndpoint,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        // Reattach the exact persisted kernel PTY whenever possible.  CWD is
        // only a deterministic first-attach hint; it is never a reconnect key.
        let info = ridge_kernel::client::list_domain_ptys(&endpoint)
            .map_err(|error| anyhow::anyhow!("list kernel PTYs: {error}"))?;
        let binding_path = remote_pty_binding_path()?;
        let binding = read_remote_pty_binding(&binding_path);
        let selected = select_kernel_pty(&info, binding.as_ref(), cwd);
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
        let after_seq = selected.as_ref().map(|pty| pty.next_seq.saturating_sub(1));
        let lease_id = ridge_kernel::client::attach_domain_pty_output(&endpoint, id, after_seq)
            .map_err(|error| anyhow::anyhow!("attach kernel PTY output: {error}"))?;
        if let Err(error) = write_remote_pty_binding(
            &binding_path,
            &RemotePtyBinding {
                schema: REMOTE_PTY_BINDING_SCHEMA,
                pty_id: id,
                workspace_id: selected.as_ref().and_then(|pty| pty.workspace_id),
                cwd: selected
                    .as_ref()
                    .and_then(|pty| pty.cwd.clone())
                    .or_else(|| cwd.map(str::to_owned)),
            },
        ) {
            let _ = ridge_kernel::client::detach_domain_pty_output(&endpoint, id, lease_id);
            return Err(error);
        }
        let (tx, rx) = mpsc::channel(64);
        let poll_endpoint = endpoint.clone();
        tokio::spawn(async move {
            loop {
                let result = tokio::task::spawn_blocking({
                    let endpoint = poll_endpoint.clone();
                    move || {
                        ridge_kernel::client::poll_domain_pty_output(
                            &endpoint, id, lease_id, 1000, 64,
                        )
                    }
                })
                .await;
                match result {
                    Ok(Ok(ridge_kernel::client::KernelPtyOutput::Data(bytes)))
                        if !bytes.is_empty() =>
                    {
                        if tx.send(bytes).await.is_err() {
                            break;
                        }
                    }
                    Ok(Ok(ridge_kernel::client::KernelPtyOutput::Timeout)) => {}
                    Ok(Ok(ridge_kernel::client::KernelPtyOutput::Lagged)) => {
                        let _ = ridge_kernel::client::resync_domain_pty_output(
                            &poll_endpoint,
                            id,
                            lease_id,
                        );
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
        Ok((
            Self {
                backend: PtyBackend::Kernel {
                    endpoint,
                    id,
                    lease_id,
                },
            },
            rx,
        ))
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        match &self.backend {
            PtyBackend::Local { registry, id } => registry.write(*id, data),
            PtyBackend::Kernel { endpoint, id, .. } => {
                ridge_kernel::client::write_domain_pty(endpoint, *id, data)
                    .map_err(|error| anyhow::anyhow!(error))
            }
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        match &self.backend {
            PtyBackend::Local { registry, id } => registry.resize(*id, cols, rows),
            PtyBackend::Kernel { endpoint, id, .. } => {
                ridge_kernel::client::resize_domain_pty(endpoint, *id, cols, rows)
                    .map_err(|error| anyhow::anyhow!(error))
            }
        }
    }
}

fn remote_pty_binding_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(REMOTE_PTY_BINDING_FILE_ENV) {
        return Ok(PathBuf::from(path));
    }
    let auth = crate::config::auth_path()
        .map_err(|error| anyhow::anyhow!("resolve Remote PTY binding path: {error}"))?;
    Ok(auth.with_file_name("remote-cloud-pane.json"))
}

fn read_remote_pty_binding(path: &PathBuf) -> Option<RemotePtyBinding> {
    let raw = fs::read(path).ok()?;
    let binding = serde_json::from_slice::<RemotePtyBinding>(&raw).ok()?;
    (binding.schema == REMOTE_PTY_BINDING_SCHEMA).then_some(binding)
}

fn write_remote_pty_binding(path: &PathBuf, binding: &RemotePtyBinding) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(binding)?)?;
    match fs::rename(&tmp, path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_file(path)?;
            fs::rename(tmp, path)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn is_live_pty(pty: &ridge_kernel::client::KernelPtyInfo) -> bool {
    pty.status == "running" || pty.status == "alive" || pty.status == "Idle"
}

/// Select a kernel PTY without fuzzy reconnect behaviour.
///
/// Persisted pane identity wins.  A cloud-specific launch profile is the
/// deterministic first-attach marker.  CWD is only a stable tie-break hint;
/// if no evidence exists, return `None` so the caller creates a new PTY rather
/// than stealing an unrelated live pane.
fn select_kernel_pty<'a>(
    info: &'a [ridge_kernel::client::KernelPtyInfo],
    binding: Option<&RemotePtyBinding>,
    cwd: Option<&str>,
) -> Option<ridge_kernel::client::KernelPtyInfo> {
    if let Some(binding) = binding {
        if let Some(pty) = info.iter().find(|pty| {
            is_live_pty(pty)
                && pty.pty_id == binding.pty_id
                && binding
                    .workspace_id
                    .map_or(true, |workspace_id| pty.workspace_id == Some(workspace_id))
        }) {
            return Some(pty.clone());
        }
    }
    info.iter()
        .filter(|pty| is_live_pty(pty) && pty.launch_profile.as_deref() == Some(REMOTE_PTY_PROFILE))
        .min_by_key(|pty| pty.pty_id)
        .cloned()
        .or_else(|| {
            let cwd = cwd.filter(|value| !value.trim().is_empty())?;
            info.iter()
                .filter(|pty| is_live_pty(pty) && pty.cwd.as_deref() == Some(cwd))
                .min_by_key(|pty| pty.pty_id)
                .cloned()
        })
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        if let PtyBackend::Kernel {
            endpoint,
            id,
            lease_id,
        } = &self.backend
        {
            let _ = ridge_kernel::client::detach_domain_pty_output(endpoint, *id, *lease_id);
        } else if let PtyBackend::Local { registry, id } = &self.backend {
            let _ = registry.destroy(*id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(
        id: Uuid,
        profile: Option<&str>,
        cwd: Option<&str>,
    ) -> ridge_kernel::client::KernelPtyInfo {
        ridge_kernel::client::KernelPtyInfo {
            id,
            pty_id: id,
            workspace_id: None,
            role: "shell".into(),
            program: None,
            launch_profile: profile.map(str::to_owned),
            cwd: cwd.map(str::to_owned),
            status: "running".into(),
            child_pid: None,
            cols: 80,
            rows: 24,
            oldest_seq: 0,
            next_seq: 1,
        }
    }

    #[test]
    fn persisted_binding_wins_over_cwd_and_profile() {
        let exact = Uuid::from_u128(2);
        let profile = info(
            Uuid::from_u128(1),
            Some(REMOTE_PTY_PROFILE),
            Some("C:\\work"),
        );
        let exact_info = info(exact, None, Some("C:\\other"));
        let binding = RemotePtyBinding {
            schema: REMOTE_PTY_BINDING_SCHEMA,
            pty_id: exact,
            workspace_id: None,
            cwd: Some("C:\\other".into()),
        };
        let selected = select_kernel_pty(
            &[profile, exact_info.clone()],
            Some(&binding),
            Some("C:\\work"),
        );
        assert_eq!(selected.map(|pty| pty.pty_id), Some(exact));
    }

    #[test]
    fn persisted_binding_does_not_cross_workspace() {
        let exact = Uuid::from_u128(2);
        let mut exact_info = info(exact, None, Some("C:\\other"));
        exact_info.workspace_id = Some(Uuid::from_u128(20));
        let binding = RemotePtyBinding {
            schema: REMOTE_PTY_BINDING_SCHEMA,
            pty_id: exact,
            workspace_id: Some(Uuid::from_u128(21)),
            cwd: Some("C:\\other".into()),
        };
        assert!(select_kernel_pty(&[exact_info], Some(&binding), None).is_none());
    }

    #[test]
    fn no_binding_never_steals_unrelated_first_pty() {
        let unrelated = info(Uuid::from_u128(1), None, Some("C:\\other"));
        assert!(select_kernel_pty(&[unrelated], None, Some("C:\\missing")).is_none());
    }

    #[test]
    fn same_cwd_selection_is_deterministic() {
        let higher = info(Uuid::from_u128(2), None, Some("C:\\work"));
        let lower = info(Uuid::from_u128(1), None, Some("C:\\work"));
        let selected = select_kernel_pty(&[higher, lower], None, Some("C:\\work")).unwrap();
        assert_eq!(selected.pty_id, Uuid::from_u128(1));
    }
}
