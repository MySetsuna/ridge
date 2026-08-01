//! Shell-independent local PTY primitive shared by kernel hosts.

use std::io::{Read, Write};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use ridge_term::term::terminal::Terminal;
use tokio::sync::mpsc;
use uuid::Uuid;

const READ_BUF: usize = 8192;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const SCROLLBACK_CAP: usize = 1024 * 1024;
const RENDER_SCROLLBACK_ROWS: usize = 4096;

/// A PTY's process handles and ordered output stream. It has no UI, RPC, or
/// Tauri dependency; lifecycle owners decide how to route its output.
pub struct PtyBridge {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

/// Kernel-side PTY lifecycle authority. Shells may retain an output receiver,
/// but every write, resize, and destroy resolves through this registry.
#[derive(Default)]
pub struct PtyRegistry {
    ptys: Mutex<HashMap<Uuid, ManagedPty>>,
    next_index: AtomicUsize,
}

struct ManagedPty {
    bridge: Arc<PtyBridge>,
    scrollback: Arc<Mutex<Vec<u8>>>,
    renderer: Arc<Mutex<Terminal>>,
    closing: std::sync::atomic::AtomicBool,
    info: PtyInfo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyInfo {
    pub id: Uuid,
    pub pane_index: usize,
    pub workspace_id: Option<Uuid>,
    pub role: String,
    pub launch_profile: Option<String>,
    pub cwd: Option<String>,
    pub status: String,
    pub cols: u16,
    pub rows: u16,
}

impl PtyRegistry {
    pub fn spawn(
        &self,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.spawn_command_for(id, shell, &[], cwd, None, "shell", None)
    }

    pub fn spawn_command_for(
        &self,
        id: Uuid,
        program: Option<&str>,
        args: &[String],
        cwd: Option<&str>,
        workspace_id: Option<Uuid>,
        role: &str,
        launch_profile: Option<&str>,
    ) -> Result<Uuid> {
        if self.contains(id) {
            anyhow::bail!("PTY already exists: {id}");
        }
        let (bridge, output) = PtyBridge::spawn_command(program, args, cwd)?;
        let bridge = Arc::new(bridge);
        let scrollback = Arc::new(Mutex::new(Vec::new()));
        let renderer = Arc::new(Mutex::new(Terminal::new(
            DEFAULT_ROWS as usize,
            DEFAULT_COLS as usize,
            RENDER_SCROLLBACK_ROWS,
        )));
        let sink = scrollback.clone();
        let screen = renderer.clone();
        tokio::spawn(async move {
            let mut output = output;
            while let Some(bytes) = output.recv().await {
                screen.lock().feed(&bytes);
                let mut retained = sink.lock();
                retained.extend_from_slice(&bytes);
                if retained.len() > SCROLLBACK_CAP {
                    let drop_count = retained.len() - SCROLLBACK_CAP;
                    retained.drain(..drop_count);
                }
            }
        });
        let info = PtyInfo {
            id,
            pane_index: self.next_index.fetch_add(1, Ordering::Relaxed),
            workspace_id,
            role: role.to_string(),
            launch_profile: launch_profile.map(str::to_string),
            cwd: cwd.map(str::to_string),
            status: "Idle".to_string(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        };
        self.ptys.lock().insert(
            id,
            ManagedPty {
                bridge,
                scrollback,
                renderer,
                closing: std::sync::atomic::AtomicBool::new(false),
                info,
            },
        );
        Ok(id)
    }

    /// Create a managed PTY while handing its single lossless output stream to
    /// a shell projection. The registry still owns write/resize/destroy.
    pub fn spawn_with_output(
        &self,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Uuid, mpsc::Receiver<Vec<u8>>)> {
        let (bridge, output) = PtyBridge::spawn(shell, cwd)?;
        let id = Uuid::new_v4();
        self.ptys.lock().insert(
            id,
            ManagedPty {
                bridge: Arc::new(bridge),
                scrollback: Arc::new(Mutex::new(Vec::new())),
                renderer: Arc::new(Mutex::new(Terminal::new(
                    DEFAULT_ROWS as usize,
                    DEFAULT_COLS as usize,
                    RENDER_SCROLLBACK_ROWS,
                ))),
                closing: std::sync::atomic::AtomicBool::new(false),
                info: PtyInfo {
                    id,
                    pane_index: self.next_index.fetch_add(1, Ordering::Relaxed),
                    workspace_id: None,
                    role: "shell".to_string(),
                    launch_profile: None,
                    cwd: cwd.map(str::to_string),
                    status: "Idle".to_string(),
                    cols: DEFAULT_COLS,
                    rows: DEFAULT_ROWS,
                },
            },
        );
        Ok((id, output))
    }

    pub fn write(&self, id: Uuid, data: &[u8]) -> Result<()> {
        self.get(id)?.write_input(data)
    }

    pub fn resize(&self, id: Uuid, cols: u16, rows: u16) -> Result<()> {
        let bridge = self.get(id)?;
        bridge.resize(cols, rows)?;
        let mut ptys = self.ptys.lock();
        let managed = ptys
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        managed.renderer.lock().resize(rows as usize, cols as usize);
        managed.info.cols = cols;
        managed.info.rows = rows;
        Ok(())
    }

    pub fn destroy(&self, id: Uuid) -> Result<()> {
        let bridge = self.begin_destroy(id)?;
        if let Err(error) = bridge.destroy() {
            self.cancel_destroy(id);
            return Err(error);
        }
        self.finish_destroy(id)?;
        Ok(())
    }

    /// Linearize pane destruction before the irreversible child kill. After
    /// this point new writes/resizes fail; callers may cancel only if kill or
    /// persistence fails, keeping the registry entry for retry.
    pub fn begin_destroy(&self, id: Uuid) -> Result<Arc<PtyBridge>> {
        let ptys = self.ptys.lock();
        let managed = ptys
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        if managed
            .closing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            anyhow::bail!("PTY is already closing: {id}");
        }
        Ok(managed.bridge.clone())
    }

    pub fn finish_destroy(&self, id: Uuid) -> Result<()> {
        let removed = self.ptys.lock().remove(&id);
        if removed.is_none() {
            anyhow::bail!("PTY not found: {id}");
        }
        Ok(())
    }

    pub fn cancel_destroy(&self, id: Uuid) {
        if let Some(managed) = self.ptys.lock().get(&id) {
            managed.closing.store(false, Ordering::Release);
        }
    }

    pub fn contains(&self, id: Uuid) -> bool {
        self.ptys.lock().contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.ptys.lock().len()
    }

    pub fn info(&self, id: Uuid) -> Result<PtyInfo> {
        self.ptys
            .lock()
            .get(&id)
            .map(|managed| managed.info.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))
    }

    pub fn list(&self) -> Vec<PtyInfo> {
        let mut entries = self
            .ptys
            .lock()
            .values()
            .map(|managed| managed.info.clone())
            .collect::<Vec<_>>();
        entries.sort_by_key(|info| info.pane_index);
        entries
    }

    pub fn set_status(&self, id: Uuid, status: &str) -> Result<()> {
        let mut ptys = self.ptys.lock();
        let managed = ptys
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        managed.info.status = status.to_string();
        Ok(())
    }

    pub fn scrollback(&self, id: Uuid, max_bytes: usize) -> Result<Vec<u8>> {
        let scrollback = self
            .ptys
            .lock()
            .get(&id)
            .map(|managed| managed.scrollback.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        let retained = scrollback.lock();
        let start = retained.len().saturating_sub(max_bytes.min(SCROLLBACK_CAP));
        Ok(retained[start..].to_vec())
    }

    /// Release retained output for a live pane. This is the backend half of a
    /// terminal clear; callers still clear their renderer separately.
    pub fn clear_scrollback(&self, id: Uuid) -> Result<()> {
        let managed = self
            .ptys
            .lock()
            .get(&id)
            .map(|managed| (managed.scrollback.clone(), managed.renderer.clone()))
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        managed.0.lock().clear();
        managed.1.lock().clear_terminal();
        Ok(())
    }

    pub fn rendered(&self, id: Uuid, lines: usize) -> Result<String> {
        let renderer = self
            .ptys
            .lock()
            .get(&id)
            .map(|managed| managed.renderer.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        let mut rows = renderer.lock().dump_visible_text();
        while rows.last().is_some_and(|line| line.trim().is_empty()) {
            rows.pop();
        }
        let start = rows.len().saturating_sub(lines.max(1));
        Ok(rows[start..].join("\n"))
    }

    fn get(&self, id: Uuid) -> Result<Arc<PtyBridge>> {
        self.ptys
            .lock()
            .get(&id)
            .filter(|managed| !managed.closing.load(Ordering::Acquire))
            .map(|managed| managed.bridge.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))
    }
}

impl Drop for PtyRegistry {
    fn drop(&mut self) {
        let bridges = std::mem::take(self.ptys.get_mut());
        for managed in bridges.into_values() {
            let _ = managed.bridge.destroy();
        }
    }
}

impl PtyBridge {
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        Self::spawn_command(shell, &[], cwd)
    }

    pub fn spawn_command(
        program: Option<&str>,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;
        let program = resolve_shell(program);
        let mut command = CommandBuilder::new(&program);
        command.args(args);
        if let Some(dir) = cwd {
            command.cwd(dir);
        }
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
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
        let (tx, rx) = mpsc::channel(256);
        spawn_reader_thread(reader, tx);
        Ok((
            Self {
                writer: Arc::new(Mutex::new(writer)),
                master: Arc::new(Mutex::new(pair.master)),
                child: Mutex::new(child),
            },
            rx,
        ))
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock();
        writer.write_all(data).context("PTY write failed")?;
        writer.flush().context("PTY flush failed")?;
        Ok(())
    }

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

    /// Explicitly stop the child before releasing the PTY handles. Dropping a
    /// handle alone is not a lifecycle guarantee on every platform.
    pub fn destroy(&self) -> Result<()> {
        let mut child = self.child.lock();
        if child.try_wait().context("PTY status failed")?.is_some() {
            return Ok(());
        }
        let pid = child.process_id();
        child.kill().context("PTY kill failed")?;
        if let Some(pid) = pid {
            ridge_core::process_guard::kill_process_tree(pid);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if child.try_wait().context("PTY reap status failed")?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                anyhow::bail!("PTY process did not exit within 2s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        // Best effort only: explicit `destroy` surfaces an operational error,
        // while drop must never panic during registry or kernel shutdown.
        let _ = self.child.get_mut().kill();
    }
}

fn resolve_shell(shell: Option<&str>) -> String {
    if let Some(shell) = shell.filter(|value| !value.trim().is_empty()) {
        return shell.to_string();
    }
    #[cfg(unix)]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            if !shell.is_empty() {
                return shell;
            }
        }
        for candidate in ["/bin/bash", "/usr/bin/bash", "/bin/zsh", "/bin/sh"] {
            if std::path::Path::new(candidate).exists() {
                return candidate.to_string();
            }
        }
        "/bin/sh".to_string()
    }
    #[cfg(windows)]
    {
        for candidate in ["pwsh.exe", "powershell.exe"] {
            if which_on_path(candidate) {
                return candidate.to_string();
            }
        }
        std::env::var("COMSPEC")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }
}

#[cfg(windows)]
fn which_on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(exe).is_file()))
        .unwrap_or(false)
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    std::thread::Builder::new()
        .name("ridge-kernel-pty-reader".to_string())
        .spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(size) if tx.blocking_send(buf[..size].to_vec()).is_err() => break,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(target: "ridge_kernel::pty", %error, "PTY reader exiting");
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
    fn resolve_shell_prefers_explicit_value() {
        assert_eq!(resolve_shell(Some("custom-shell")), "custom-shell");
        assert!(!resolve_shell(Some(" ")).is_empty());
    }

    #[test]
    fn empty_registry_has_no_pane() {
        let registry = PtyRegistry::default();
        assert_eq!(registry.len(), 0);
        assert!(!registry.contains(Uuid::new_v4()));
        assert!(registry.clear_scrollback(Uuid::new_v4()).is_err());
    }
}
