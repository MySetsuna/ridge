//! Shared test harness for `ridge-acp` E2E tests.
//!
//! - `spawn_fake_session()` — spawns `fake_acp_agent`, subscribes BEFORE
//!   `connect()` runs (so `SessionCreated` / `StateChanged` are not lost),
//!   drains the synthetic host events, and returns the connected
//!   `AgentSession` plus the active event receiver.
//! - `collect_until_terminal()` — drains the broadcast channel into a Vec
//!   until a terminal event (`TurnCompleted`, `TurnFailed`, `SessionFailed`)
//!   arrives or the timeout elapses.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::time::timeout;

use ridge_acp::provider::ProviderSpec;
use ridge_acp::spawn::{spawn_acp_agent, SpawnSpec};
use ridge_acp::transport::Framing;
use ridge_acp::{AgentSession, AgentSessionConfig, AgentSessionEvent};

fn fake_spawn_spec() -> SpawnSpec {
    // `CARGO_BIN_EXE_<name>` is set by Cargo for integration tests.
    let program = PathBuf::from(env!("CARGO_BIN_EXE_fake_acp_agent"));
    SpawnSpec {
        program,
        args: Vec::new(),
        cwd: None,
        provider: ProviderSpec::mock_default(),
        framing: Framing::ContentLength,
    }
}

pub struct FakeHarness {
    pub session: AgentSession,
    /// Receiver subscribed before `connect()` ran. Has SessionCreated /
    /// StateChanged already buffered.
    pub rx: broadcast::Receiver<AgentSessionEvent>,
}

pub async fn spawn_fake_session() -> FakeHarness {
    let spec = fake_spawn_spec();
    let (transport, child) = spawn_acp_agent(&spec).expect("spawn fake_acp_agent");
    let (session, rx) = AgentSession::prepare(AgentSessionConfig::mock_default());
    session
        .connect_into(transport, child)
        .await
        .expect("connect_into");
    FakeHarness { session, rx }
}

pub async fn collect_until_terminal(
    session: &AgentSession,
    dur: Duration,
) -> Vec<AgentSessionEvent> {
    let mut rx = session.subscribe();
    let mut out = Vec::new();
    let _ = timeout(dur, async {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let terminal = matches!(
                        ev,
                        AgentSessionEvent::TurnCompleted { .. }
                            | AgentSessionEvent::TurnFailed { .. }
                            | AgentSessionEvent::SessionFailed { .. }
                    );
                    out.push(ev);
                    if terminal {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
    .await;
    out
}
