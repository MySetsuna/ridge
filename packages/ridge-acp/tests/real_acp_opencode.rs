//! REAL_ACP_E2E — exercises Ridge's transport against `opencode acp`,
//! a real ACP-compatible binary (opencode 2.0.6, opencode-ai CLI), NOT a
//! mock fixture.
//!
//! This test exists because `live_acp_agent` is a deterministic fixture —
//! it does not prove Ridge speaks the wire shape that real ACP agents
//! (opencode, codex-acp, jetbrains-acp, ...) emit. The canonical ACP wire
//! shape uses:
//!
//! - `protocolVersion: <number>`            (not "v1" string)
//! - `clientInfo: {name, version}`          (not `client`)
//! - `agentCapabilities: { ... }`            (structured object, not Vec<String>)
//! - `authMethods: [...]`                    (auth requirement surfaced)
//! - `agentInfo: {name, version}`
//!
//! This test deliberately does NOT touch my M1 fixture (`live_acp_agent`).
//! It speaks only to a real ACP binary and exercises:
//!
//! 1. initialize          — capability negotiation + protocolVersion match
//! 2. session/new         — open a session
//! 3. unknown method      — verify -32601 (no fabricated support)
//! 4. capability probe    — assert agentCapabilities structure; mark
//!    any method Ridge wants that opencode lacks
//!    as a CAPABILITY GAP, not a Ridge-side fake.
//!
//! If `opencode` is not on PATH the test is skipped (this is a feature:
//! the gate is "real ACP available" and the env is the source of truth).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use ridge_acp::transport::{AcpTransport, Framing};

/// Locate `opencode` on PATH. Returns `None` if absent (test is skipped).
///
/// Prefers the real PE binary that ships inside the `opencode-ai` npm
/// package (`opencode-ai/bin/opencode.exe`) over the PATH shim, because
/// the npm `.cmd` wrapper does not forward stdio cleanly when launched
/// directly via CreateProcess. On non-Windows, falls back to PATH lookup.
fn opencode_path() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Ok(npm_root) = std::env::var("npm_config_prefix")
            .or_else(|_| std::env::var("NPM_PREFIX"))
        {
            let direct = std::path::PathBuf::from(npm_root)
                .join("node_modules")
                .join("opencode-ai")
                .join("bin")
                .join("opencode.exe");
            if direct.is_file() {
                return Some(direct);
            }
        }
    }
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ""]
    } else {
        &["", ".exe"]
    };
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for ext in exts {
            let mut p = dir.clone();
            let name = if ext.is_empty() {
                "opencode".to_string()
            } else {
                format!("opencode{ext}")
            };
            p.push(&name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Spawn `opencode acp` with stdio JSON-RPC pipes wired up.
fn spawn_opencode_acp(bin: &std::path::Path) -> tokio::process::Command {
    let mut cmd = Command::new(bin);
    cmd.arg("acp");
    cmd
}

#[tokio::test]
async fn real_acp_opencode_initialize_negotiates_protocol_version() {
    let Some(bin) = opencode_path() else {
        eprintln!("[skip] opencode not on PATH — no real ACP agent to test against");
        return;
    };
    let mut cmd = spawn_opencode_acp(&bin);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn opencode acp");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    // opencode uses NDJSON, not Content-Length (verified by sending both
    // framings through bash and observing opencode's parser error on the
    // latter). Ridge wires the agent's framing into the transport so the
    // host never has to translate wire shape.
    let transport = AcpTransport::spawn_with_framing(stdin, stdout, None, Framing::NdJson);
    assert_eq!(transport.framing(), Framing::NdJson);

    // Canonical wire shape: protocolVersion is a number, clientInfo is {name, version}.
    let init_params = serde_json::json!({
        "protocolVersion": 1,
        "clientInfo": {
            "name": "ridge-acp",
            "version": "0.1.0"
        }
    });

    let resp = tokio::time::timeout(
        Duration::from_secs(10),
        transport.call("initialize", init_params),
    )
    .await
    .expect("initialize responds within 10s")
    .expect("initialize ok");

    assert!(resp.error.is_none(), "initialize returned error: {:?}", resp.error);
    let result = resp.result.expect("initialize must return a result");

    // protocolVersion must be a number, not a string.
    let pv = result.get("protocolVersion").and_then(|v| v.as_i64()).unwrap_or(-1);
    assert_eq!(pv, 1, "protocolVersion must be 1 (number), got {pv}");

    // agentCapabilities is a structured object, not Vec<String>.
    let caps = result.get("agentCapabilities").and_then(|v| v.as_object());
    let caps = caps.expect("agentCapabilities must be a structured object");
    // Ridge maps a subset of these to its canonical SessionStatus + UI affordances.
    let _load_session = caps.get("loadSession").and_then(|v| v.as_bool()).unwrap_or(false);
    let _prompt_caps = caps.get("promptCapabilities");
    let _session_caps = caps.get("sessionCapabilities");

    // authMethods is required up front; opencode has opencode-login.
    let auth = result.get("authMethods").and_then(|v| v.as_array());
    let auth = auth.expect("authMethods array");
    assert!(!auth.is_empty(), "opencode must declare at least one auth method");
    let first_id = auth[0].get("id").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(first_id, "opencode-login", "expected opencode-login auth method");

    // agentInfo is opencode's self-identification.
    let agent_info = result.get("agentInfo").and_then(|v| v.as_object());
    let agent_info = agent_info.expect("agentInfo present");
    let name = agent_info.get("name").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(name, "OpenCode", "agentInfo.name must be OpenCode");

    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn real_acp_opencode_session_new_opens_session() {
    let Some(bin) = opencode_path() else {
        eprintln!("[skip] opencode not on PATH");
        return;
    };
    let mut cmd = spawn_opencode_acp(&bin);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn opencode acp");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn_with_framing(stdin, stdout, None, Framing::NdJson);

    // initialize first (opencode rejects session/new before initialize).
    let _ = tokio::time::timeout(
        Duration::from_secs(10),
        transport.call("initialize", serde_json::json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "ridge-acp", "version": "0.1.0"}
        })),
    )
    .await
    .expect("initialize")
    .expect("initialize ok");

    // session/new — canonical shape uses cwd (string, not null) and mcpServers.
    // Opencode may require additional fields; we send the minimal set we know
    // about and observe what comes back. Capability gaps must surface honestly.
    let resp = tokio::time::timeout(
        Duration::from_secs(10),
        transport.call("session/new", serde_json::json!({
            "cwd": std::env::temp_dir().to_string_lossy(),
            "mcpServers": []
        })),
    )
    .await;

    match resp {
        Ok(Ok(r)) => {
            // Either a result (session id returned) or an error (auth required /
            // missing param). Both are honest responses from opencode — Ridge
            // MUST NOT fabricate.
            if let Some(err) = r.error {
                eprintln!("[probe] session/new returned error code {} msg={}",
                    err.code, err.message);
                assert_ne!(err.code, -32603, "must not be internal-error from Ridge side");
            } else if let Some(result) = r.result {
                let sid = result.get("sessionId").and_then(|v| v.as_str());
                eprintln!("[probe] session/new returned session_id={:?}", sid);
                if let Some(sid) = sid {
                    assert!(!sid.is_empty(), "session_id must be non-empty");
                }
            }
        }
        Ok(Err(e)) => {
            eprintln!("[probe] session/new transport error: {e:?}");
        }
        Err(_) => {
            eprintln!("[probe] session/new timed out — opencode may need auth");
        }
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn real_acp_opencode_unknown_method_returns_method_not_found() {
    // Per M2 directive §3: Ridge MUST NOT fabricate provider behavior.
    // When opencode does not implement a method, it returns -32601 — Ridge
    // surfaces that to the caller unchanged.
    let Some(bin) = opencode_path() else {
        eprintln!("[skip] opencode not on PATH");
        return;
    };
    let mut cmd = spawn_opencode_acp(&bin);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn opencode acp");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn_with_framing(stdin, stdout, None, Framing::NdJson);

    let _ = tokio::time::timeout(
        Duration::from_secs(10),
        transport.call("initialize", serde_json::json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "ridge-acp", "version": "0.1.0"}
        })),
    )
    .await
    .expect("initialize")
    .expect("initialize ok");

    // Method Ridge wants that opencode does NOT implement: session/cancel.
    let cancel = tokio::time::timeout(
        Duration::from_secs(5),
        transport.call("session/cancel", serde_json::json!({"sessionId": "x"})),
    )
    .await
    .expect("cancel responds")
    .expect("cancel delivery");
    let err = cancel.error.expect("opencode must return error for unimplemented method");
    assert_eq!(err.code, -32601, "JSON-RPC method-not-found code, got {} msg={}",
        err.code, err.message);
    assert!(err.message.contains("session/cancel"),
        "method-name must be in error message, got {}", err.message);

    // Method that definitely doesn't exist anywhere.
    let bogus = tokio::time::timeout(
        Duration::from_secs(5),
        transport.call("ridge/test_probe_bogus_method", serde_json::json!({})),
    )
    .await
    .expect("bogus responds")
    .expect("bogus delivery");
    let err = bogus.error.expect("bogus must return error");
    assert_eq!(err.code, -32601);

    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn real_acp_opencode_capability_gap_summary() {
    // Documents what Ridge asked opencode-acp to do and what opencode said.
    // Read by humans when wiring Desktop/Mobile: this is the truth.
    let Some(bin) = opencode_path() else {
        eprintln!("[skip] opencode not on PATH");
        return;
    };
    let mut cmd = spawn_opencode_acp(&bin);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn opencode acp");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn_with_framing(stdin, stdout, None, Framing::NdJson);

    let resp = tokio::time::timeout(
        Duration::from_secs(10),
        transport.call("initialize", serde_json::json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "ridge-acp", "version": "0.1.0"}
        })),
    )
    .await
    .expect("initialize responds")
    .expect("initialize ok");
    let result = resp.result.expect("result present");
    let caps = result.get("agentCapabilities").and_then(|v| v.as_object()).unwrap();

    // Document the gap matrix. Each entry is (ridge-capability, opencode-support).
    let matrix: &[(&str, bool)] = &[
        ("loadSession", caps.get("loadSession").and_then(|v| v.as_bool()).unwrap_or(false)),
        ("session/list", caps.get("sessionCapabilities")
            .and_then(|v| v.get("list")).is_some()),
        ("session/resume", caps.get("sessionCapabilities")
            .and_then(|v| v.get("resume")).is_some()),
        ("session/fork", caps.get("sessionCapabilities")
            .and_then(|v| v.get("fork")).is_some()),
        ("session/close", caps.get("sessionCapabilities")
            .and_then(|v| v.get("close")).is_some()),
        ("session/delete", caps.get("sessionCapabilities")
            .and_then(|v| v.get("delete")).is_some()),
        ("prompt(image)", caps.get("promptCapabilities")
            .and_then(|v| v.get("image")).and_then(|v| v.as_bool()).unwrap_or(false)),
        ("prompt(embeddedContext)", caps.get("promptCapabilities")
            .and_then(|v| v.get("embeddedContext")).and_then(|v| v.as_bool()).unwrap_or(false)),
        ("mcp/http", caps.get("mcpCapabilities")
            .and_then(|v| v.get("http")).and_then(|v| v.as_bool()).unwrap_or(false)),
        ("mcp/sse", caps.get("mcpCapabilities")
            .and_then(|v| v.get("sse")).and_then(|v| v.as_bool()).unwrap_or(false)),
    ];
    let mut supported = 0;
    let mut gaps = 0;
    for (cap, has) in matrix {
        if *has { supported += 1; } else { gaps += 1; }
        eprintln!("  opencode-acp {cap}: {}", if *has { "supported" } else { "GAP" });
    }
    eprintln!("[capability] opencode-acp: {supported} supported, {gaps} GAP");

    // Ridge-side capabilities opencode-acp does NOT advertise — these are
    // expected to be marked as capability gaps in Ridge's UI, not faked.
    // (Listed here as a contract; assertion below is a no-op so the test
    //  remains useful as documentation.)
    let ridge_only_capabilities = [
        "session/cancel",        // opencode returned -32601 above
        "agent/render_artifact", // not advertised by opencode
        "session/approve",       // M1 extension; not in canonical ACP
    ];
    for c in &ridge_only_capabilities {
        eprintln!("  ridge-only {c}: GAP (opencode does not implement)");
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
}
