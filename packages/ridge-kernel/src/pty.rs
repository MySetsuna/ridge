//! Shell-independent local PTY primitive shared by kernel hosts.

use std::io::{Read, Write};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

const READ_BUF: usize = 8192;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// A PTY's process handles and ordered output stream. It has no UI, RPC, or
/// Tauri dependency; lifecycle owners decide how to route its output.
pub struct PtyBridge {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

impl PtyBridge {
    pub fn spawn(
        shell: Option<&str>,
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
        let program = resolve_shell(shell);
        let mut command = CommandBuilder::new(&program);
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
        self.child.lock().kill().context("PTY kill failed")
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
}
