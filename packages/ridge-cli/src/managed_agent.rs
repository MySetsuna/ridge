use crate::tui::Session;
use anyhow::{anyhow, Context, Result};
use std::io::{IsTerminal, Read, Write};
use tokio::sync::mpsc;

pub(crate) use ridge_kernel::managed_console::AgentArgs;

pub(crate) async fn run(args: AgentArgs) -> Result<()> {
    let (session, output, endpoint) = crate::tui::lan_session::connect_collaboration(args).await?;
    let target = session
        .pane_ref()
        .context("collaboration terminal identity unavailable")?;
    eprintln!(
        "Collaboration terminal workspace={} pane={}",
        target.workspace_id, target.pane_id
    );
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return crate::tui::run_session(session, output).await;
    }
    run_piped(session, output, endpoint, target).await
}

async fn run_piped(
    session: crate::tui::lan_session::LanControllerSession,
    mut output: mpsc::Receiver<Vec<u8>>,
    endpoint: ridge_kernel::registry::KernelEndpoint,
    target: ridge_term::terminal_protocol::PaneRef,
) -> Result<()> {
    let (sender, mut input) = mpsc::channel(8);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut bytes = [0; 1024];
        let mut previous_cr = false;
        loop {
            match stdin.read(&mut bytes) {
                Ok(0) => break,
                Ok(size) => {
                    // A piped line ending must produce the same Enter as the terminal keymap.
                    let mut line = Vec::with_capacity(size);
                    for &byte in &bytes[..size] {
                        if byte != b'\n' || !previous_cr {
                            line.push(if byte == b'\n' { b'\r' } else { byte });
                        }
                        previous_cr = byte == b'\r';
                    }
                    if sender.blocking_send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.blocking_send(Err(error));
                    break;
                }
            }
        }
    });
    let mut poll = tokio::time::interval(std::time::Duration::from_millis(250));
    let mut exited_at = None;
    loop {
        tokio::select! {
            Some(bytes) = input.recv(), if session.input_ready() => session.send_input(&bytes?)?,
            Some(bytes) = output.recv() => {
                std::io::stdout().write_all(&bytes)?;
                std::io::stdout().flush()?;
            }
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = poll.tick() => {
                if let Some(error) = session.failure() { return Err(anyhow!(error)); }
                let endpoint = endpoint.clone();
                let snapshot = tokio::task::spawn_blocking(move || ridge_kernel::client::request_json(
                    &endpoint, "GET", "/v1/domain/ptys", None)).await?.map_err(anyhow::Error::msg)?;
                let terminal = snapshot["ptys"].as_array().and_then(|ptys| ptys.iter().find(|pty|
                    pty["workspace_id"] == target.workspace_id && pty["id"] == target.pane_id))
                    .context("collaboration terminal no longer available; reconcile the original outcome")?;
                if terminal["status"] == "Exited" {
                    let since = exited_at.get_or_insert_with(std::time::Instant::now);
                    if let Some(code) = terminal["exit_code"].as_i64() {
                        if let Some(bytes) = session.presentation() { std::io::stdout().write_all(&bytes)?; }
                        std::io::stdout().flush()?;
                        anyhow::ensure!(code == 0, "collaboration console exited with code {code}; inspect its terminal output");
                        return Ok(());
                    }
                    anyhow::ensure!(since.elapsed() < std::time::Duration::from_secs(2),
                        "collaboration terminal exited; exit outcome unavailable");
                }
            }
        }
    }
}
