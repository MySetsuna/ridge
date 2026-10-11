//! Test fixture process launcher. Not exported by the production ACP library.
//! `SpawnSpec` + `spawn_acp_agent` — start an ACP-compatible child process and
//! hand back an `AcpTransport` bound to its stdio.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::Command;

use ridge_acp::error::AcpErrorWrap;
use ridge_acp::provider::ProviderSpec;
use ridge_acp::transport::{AcpTransport, Framing};

/// How to materialize one ACP-compatible child process.
///
/// `program` is the binary path (resolved by `spawn` to an absolute path when
/// possible). `args` are passed through unchanged. `cwd` is optional. The
/// `provider` field is for downstream logging / capability negotiation —
/// `spawn` itself only cares about the binary.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub provider: ProviderSpec,
    /// Wire framing. Defaults to `ContentLength` (matches the wind
    /// `live_acp_agent` fixture). Production ACP agents like
    /// `opencode acp` use `NdJson`; consumers can set this explicitly.
    pub framing: Framing,
}

impl SpawnSpec {
    /// Convenience constructor that pins `framing = ContentLength`,
    /// matching the legacy / default behavior.
    pub fn default_with_provider(provider: ProviderSpec) -> Self {
        Self {
            program: PathBuf::new(),
            args: Vec::new(),
            cwd: None,
            provider,
            framing: Framing::ContentLength,
        }
    }
}

/// Spawn the agent and return a connected transport plus a `Child` handle.
///
/// The caller MUST keep the `Child` alive for as long as it intends to talk
/// to the agent — dropping the `Child` kills the process, which closes the
/// pipes and surfaces as `SessionClosed` in the transport.
pub fn spawn_acp_agent(
    spec: &SpawnSpec,
) -> Result<(AcpTransport, tokio::process::Child), AcpErrorWrap> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| AcpErrorWrap::Spawn(format!("{}: {e}", spec.program.display())))?;
    let stdin = child.stdin.take().ok_or_else(|| {
        AcpErrorWrap::Spawn("missing child stdin".into())
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        AcpErrorWrap::Spawn("missing child stdout".into())
    })?;
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn_with_framing(stdin, stdout, _stderr, spec.framing);
    Ok((transport, child))
}
