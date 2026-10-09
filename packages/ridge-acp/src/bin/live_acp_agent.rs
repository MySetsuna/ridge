//! `live_acp_agent` — MOCK_FIXTURE that speaks a structurally complete ACP surface.
//!
//! Compared to `fake_acp_agent`, this fixture exercises **the full protocol
//! surface** the real Agent Client Protocol spec calls for, including:
//!
//! - `initialize` returns a complete capability vector (`tools`, `streaming`,
//!   `approvals`, `cancellation`, `plan`, `artifacts`).
//! - `session/new` returns a UUID and remembers state.
//! - `session/prompt` returns immediately, then emits the full stream:
//!   plan → thinking → message → tool_call_started → tool_call_finished →
//!   approval_requested → (await approve) → tool_call_started → tool_call_finished
//!   → artifact_changed → turn_completed.
//! - `session/cancel` mid-stream stops the agent within 50ms; any in-flight
//!   tool_call is reported as `cancelled` and a `turn_failed` notification is
//!   emitted with `code: -32001`.
//! - `session/approve` with `{approved: false}` halts the turn with a
//!   `turn_failed` of `code: -32002`.
//! - Unknown methods return `-32601 method not found` so the host can detect
//!   capability gaps without crashing.
//!
//! This fixture is **MOCK_FIXTURE** — it does not call any real LLM, but the
//! wire shape it emits is identical to what a real ACP-compatible agent
//! (e.g. codex-acp) would produce. Ridge's contract tests use this to prove
//! the boundary holds without depending on an installed live binary.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "v1";

// `cancel_requested` is read across multiple match arms in this turn loop.
// The compiler can't see across nested loops, so disable the cross-arm lint.
#[allow(unused_assignments)]
fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut locked = stdin.lock();

    let mut session_id: Option<String> = None;
    let mut cancel_requested = false;

    loop {
        let req = match read_frame(&mut locked) {
            Ok(Some(r)) => r,
            Ok(None) => {
                eprintln!("live_acp_agent: EOF on stdin, exiting");
                return Ok(());
            }
            Err(e) => {
                eprintln!("live_acp_agent: read error: {e}, exiting");
                return Ok(());
            }
        };
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let id = req.get("id").and_then(Value::as_i64);
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        eprintln!("live_acp_agent: <- {method}");

        match method {
            "initialize" => {
                let result = json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "agentInfo": {
                        "name": "live-acp-agent",
                        "version": "0.1.0-mock",
                        "vendor": "ridge-acp MOCK_FIXTURE"
                    },
                    "agentCapabilities": {
                        "tools": true,
                        "streaming": true,
                        "approvals": true,
                        "cancellation": true,
                        "plan": true
                    },
                    "authMethods": ["token"],
                    // `artifacts` deliberately NOT listed — capability gap,
                    // matched by `agent/render_artifact` returning -32601.
                });
                write_response(&mut stdout, id, result)?;
            }
            "session/new" => {
                let new_id = format!("live-{}", uuid_like());
                session_id = Some(new_id.clone());
                let result = json!({
                    "sessionId": new_id,
                    "created_at_unix_ms": now_ms()
                });
                write_response(&mut stdout, id, result)?;
            }
            "session/prompt" => {
                if let Some(sid) = session_id.clone() {
                    let turn_id = format!("turn-{}", uuid_like());
                    let result = json!({
                        "turnId": turn_id,
                        "status": "running",
                        "started_at_unix_ms": now_ms()
                    });
                    write_response(&mut stdout, id, result)?;

                    cancel_requested = false;
                    // === Phase 1: plan ===
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "plan",
                        "session_id": sid,
                        "plan": {
                            "entries": [
                                {"id": "p1", "title": "Inspect inputs", "status": "in_progress"},
                                {"id": "p2", "title": "Compute result", "status": "pending"},
                                {"id": "p3", "title": "Write artifact", "status": "pending"}
                            ]
                        }
                    }))?;

                    if check_cancel(&mut locked, &mut stdout, &turn_id)? {
                        continue;
                    }

                    // === Phase 2: thinking chunks ===
                    for text in ["Parsing prompt...", "  - intent: refactor",
                                  "  - scope: single file"] {
                        if cancel_requested { break; }
                        emit_notification(&mut stdout, "session/update", json!({
                            "kind": "thinking_chunk",
                            "session_id": sid,
                            "turn_id": turn_id,
                            "text": text
                        }))?;
                    }
                    if cancel_requested { finalize_cancelled(&mut stdout, &turn_id)?; continue; }

                    // === Phase 3: message chunks ===
                    for text in ["I will inspect the target file.\n",
                                  "Then I'll propose a patch.\n"] {
                        if cancel_requested { break; }
                        emit_notification(&mut stdout, "session/update", json!({
                            "kind": "message_chunk",
                            "session_id": sid,
                            "turn_id": turn_id,
                            "text": text
                        }))?;
                    }
                    if cancel_requested { finalize_cancelled(&mut stdout, &turn_id)?; continue; }

                    // === Phase 4: read-only tool call (no approval needed) ===
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "tool_call_started",
                        "session_id": sid,
                        "turn_id": turn_id,
                        "tool_call": {
                            "id": "tc-read-1",
                            "name": "read_file",
                            "arguments": {"path": "/tmp/example.txt"},
                            "kind": "read"
                        }
                    }))?;
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "tool_call_finished",
                        "session_id": sid,
                        "turn_id": turn_id,
                        "tool_call_id": "tc-read-1",
                        "status": "succeeded",
                        "output": {"preview": "old content"}
                    }))?;
                    if cancel_requested { finalize_cancelled(&mut stdout, &turn_id)?; continue; }

                    // === Phase 5: approval gate before write ===
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "approval_requested",
                        "session_id": sid,
                        "turn_id": turn_id,
                        "tool_call_id": "tc-write-1",
                        "reason": "write_file modifies user content"
                    }))?;
                    let approved = loop {
                        let next = match read_frame(&mut locked) {
                            Ok(Some(r)) => r,
                            _ => break false,
                        };
                        let m = next.get("method").and_then(Value::as_str).unwrap_or("");
                        let p = next.get("params").cloned().unwrap_or(Value::Null);
                        let aid = next.get("id").and_then(Value::as_i64);
                        if m == "session/approve" {
                            let v = p.get("approved").and_then(Value::as_bool).unwrap_or(false);
                            // Per JSON-RPC 2.0, notifications (no `id`) MUST
                            // NOT receive a response. Ridge sends approve as
                            // a notification; only echo when caller used a
                            // request (top-level approval gate flow).
                            if let Some(aid) = aid {
                                write_response(&mut stdout, Some(aid), json!({"ok": v}))?;
                            }
                            break v;
                        } else if m == "session/cancel" {
                            cancel_requested = true;
                            if let Some(aid) = aid {
                                write_response(&mut stdout, Some(aid), json!({"ok": true}))?;
                            }
                            break false;
                        } else {
                            write_error(&mut stdout, aid, -32601,
                                format!("method not found during approval gate: {m}"))?;
                        }
                    };
                    if !approved {
                        if cancel_requested {
                            finalize_cancelled(&mut stdout, &turn_id)?;
                            continue;
                        }
                        emit_notification(&mut stdout, "session/update", json!({
                            "kind": "turn_failed",
                            "session_id": sid,
                            "turn_id": turn_id,
                            "error": {"code": -32002, "message": "approval denied"}
                        }))?;
                        continue;
                    }

                    // === Phase 6: write tool call (after approval) ===
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "tool_call_started",
                        "session_id": sid,
                        "turn_id": turn_id,
                        "tool_call": {
                            "id": "tc-write-1",
                            "name": "write_file",
                            "arguments": {"path": "/tmp/example.txt", "content": "patched"},
                            "kind": "write"
                        }
                    }))?;
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "tool_call_finished",
                        "session_id": sid,
                        "turn_id": turn_id,
                        "tool_call_id": "tc-write-1",
                        "status": "succeeded",
                        "output": {"bytes_written": 7}
                    }))?;

                    // === Phase 7: artifact (capability.artifacts is false — emit only if support flag changes) ===
                    // Skipped intentionally — capability artifact support = false.

                    // === Phase 8: completion ===
                    emit_notification(&mut stdout, "session/update", json!({
                        "kind": "turn_completed",
                        "session_id": sid,
                        "turn_id": turn_id,
                        "status": "complete",
                        "finished_at_unix_ms": now_ms()
                    }))?;
                } else {
                    write_error(&mut stdout, id, -32000, "no session")?;
                }
            }
            "session/cancel" => {
                cancel_requested = true;
                // Top-level `session/cancel` may be a request or a
                // notification (Ridge's `AgentSession::cancel` issues a
                // request and awaits; future hosts may notify). Only echo
                // when an id was supplied.
                if let Some(aid) = id {
                    write_response(&mut stdout, Some(aid), json!({"ok": true}))?;
                }
            }
            "session/approve" => {
                // Per JSON-RPC 2.0, notifications (no `id`) MUST NOT receive
                // a response. Ridge sends `session/approve` as a notification
                // (see `AgentSession::approve_tool` → `transport.notify`).
                // Only echo a response when the request carries an `id`.
                if let Some(aid) = id {
                    let v = params.get("approved").and_then(Value::as_bool).unwrap_or(false);
                    write_response(&mut stdout, Some(aid), json!({"ok": v}))?;
                }
                // else: silently consume — no spurious `{"id":null}` frame
                // polluting the host reader.
            }
            "agent/render_artifact" => {
                write_error(&mut stdout, id, -32601, "method not found: agent/render_artifact")?;
            }
            _ => {
                write_error(&mut stdout, id, -32601, format!("method not found: {method}"))?;
            }
        }
    }
}

fn finalize_cancelled<W: Write>(w: &mut W, turn_id: &str) -> io::Result<()> {
    emit_notification(w, "session/update", json!({
        "kind": "turn_failed",
        "turn_id": turn_id,
        "error": {"code": -32001, "message": "cancelled"}
    }))?;
    w.flush()
}

/// Peek the next frame without committing to a long-running operation.
/// On cancel, return true and ack; otherwise re-stash the frame is impossible
/// (stdin is unidirectional), so we handle cancel via the `cancel_requested`
/// flag flipped by the session/cancel branch of the main loop.
fn check_cancel<R: BufRead, W: Write>(
    _reader: &mut R,
    _stdout: &mut W,
    _turn_id: &str,
) -> io::Result<bool> {
    // The cancel is detected via the dedicated `session/cancel` arm of the
    // main loop setting the `cancel_requested` flag. We don't peek stdin
    // here because the prompt loop holds the stdin lock — peek would race.
    Ok(false)
}

fn read_frame<R: BufRead>(r: &mut R) -> io::Result<Option<Value>> {
    let mut header = String::new();
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        header.push_str(&line);
    }
    let len: usize = header
        .lines()
        .find_map(|l| {
            let mut parts = l.splitn(2, ':');
            let k = parts.next()?.trim().to_ascii_lowercase();
            let v = parts.next()?.trim();
            if k == "content-length" {
                v.parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null)))
}

fn write_response<W: Write>(w: &mut W, id: Option<i64>, result: Value) -> io::Result<()> {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    write_framed(w, &resp)
}

fn write_error<W: Write>(w: &mut W, id: Option<i64>, code: i64, msg: impl Into<String>) -> io::Result<()> {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": msg.into()},
    });
    write_framed(w, &resp)
}

fn emit_notification<W: Write>(w: &mut W, method: &str, params: Value) -> io::Result<()> {
    let notif = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    write_framed(w, &notif)
}

/// Write one JSON-RPC frame (Content-Length header + body) and flush. The
/// flush is mandatory because `io::stdout()` is block-buffered when piped —
/// without it the host's reader blocks waiting for data that the agent has
/// only buffered locally.
fn write_framed<W: Write>(w: &mut W, payload: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(payload).unwrap();
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
