//! `fake_acp_agent` — MOCK_FIXTURE stdio JSON-RPC server for ACP.
//!
//! Speaks the minimum ACP framing `ridge-acp` expects:
//! - `initialize` → returns `InitializeResult` with capabilities
//! - `session/new` → returns a UUID `session_id`
//! - `session/prompt` → emits a deterministic sequence of `SessionUpdate`
//!   notifications: Plan, ThinkingChunk×2, MessageChunk×2, ToolCallStarted
//!   (read_file), ApprovalRequested, then waits for `session/approve` and
//!   emits ToolCallStarted (write_file), ToolCallFinished, TurnCompleted.
//! - `session/cancel` → emits TurnFailed.
//!
//! All frames are Content-Length framed over stdio. There is no network code,
//! no Tauri invoke, no shell ANSI — that is the whole point of M1: prove the
//! boundary is real without coupling to anything else.

use std::io::{self, BufRead, Write};

use serde_json::{json, Map, Value};

const PROTOCOL_VERSION: &str = "v1";

fn main() -> io::Result<()> {
    let no_approval = std::env::args().any(|arg| arg == "--no-approval");
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut locked = stdin.lock();

    let mut session_id: Option<String> = None;

    loop {
        let req = match read_frame(&mut locked) {
            Ok(Some(r)) => r,
            Ok(None) => {
                eprintln!("fake_acp_agent: EOF on stdin, exiting");
                return Ok(());
            }
            Err(e) => {
                eprintln!("fake_acp_agent: read error: {e}, exiting");
                return Ok(());
            }
        };
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        eprintln!("fake_acp_agent: <- {method}");
        let id = req.get("id").and_then(Value::as_i64);
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                let result = json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "agentInfo": {"name": "fake-acp-agent", "version": "0.1.0-mock"},
                    "agentCapabilities": {
                        "tools": true,
                        "streaming": true,
                        "approvals": true,
                    },
                    "authMethods": ["token"]
                });
                write_response(&mut stdout, id, result)?;
            }
            "session/new" => {
                let new_id = format!("sess-{}", uuid_like());
                session_id = Some(new_id.clone());
                let result = json!({ "sessionId": new_id });
                write_response(&mut stdout, id, result)?;
            }
            "session/prompt" => {
                if let Some(sid) = session_id.clone() {
                    let turn_id = format!("turn-{}", uuid_like());
                    let result = json!({
                        "turnId": turn_id,
                        "status": "complete"
                    });
                    write_response(&mut stdout, id, result)?;

                    let prompt_text = params["prompt"][0]["text"].as_str().unwrap_or("");
                    if prompt_text.starts_with("[ridge plan proposal]") || prompt_text.starts_with("Summarize the following structured") {
                        let message = if prompt_text.starts_with("[ridge plan proposal]") {
                            let tasks: Vec<_> = [("A", vec![]), ("B", vec!["A"]), ("C", vec!["A"]), ("D", vec!["B", "C"])].into_iter().map(|(id, dependencies)| json!({ "id": id, "title": format!("Fixture task {id}"), "description": format!("MOCK_FIXTURE task {id}"), "point": "small", "dependencies": dependencies, "assigned_session_id": null, "status": "queued", "result": null })).collect();
                            json!({"title":"Mock fixture collaboration", "stories":[{"id":"story-1","title":"Fixture story","description":"Deterministic acceptance only","tasks":tasks}]}).to_string()
                        } else { "Mock fixture Lead consumed the structured outcome.".into() };
                        emit_notification(&mut stdout, "session/update", json!({"kind":"message_chunk", "turn_id":turn_id, "text":message}))?;
                        emit_notification(&mut stdout, "session/update", json!({"kind":"turn_completed", "turn_id":turn_id, "status":"complete"}))?;
                        stdout.flush()?;
                        continue;
                    }
                    // Explicit fixture barrier for a headed host-restart test; never used by production providers.
                    if prompt_text.starts_with("Task C:") {
                        if let Ok(directory) = std::env::var("RIDGE_ACP_FIXTURE_CONTROL_DIR") {
                            let directory = std::path::PathBuf::from(directory);
                            std::fs::write(directory.join("C.waiting"), b"prompt accepted")?;
                            stdout.flush()?;
                            while !directory.join("C.resume").exists() { std::thread::sleep(std::time::Duration::from_millis(20)); }
                        }
                    }
                    // Emit deterministic stream on a best-effort basis.
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "plan",
                        "plan": {
                            "entries": [
                                {"id": "p1", "title": "Read source file", "status": "in_progress"},
                                {"id": "p2", "title": "Patch file", "status": "pending"},
                                {"id": "p3", "title": "Summarize change", "status": "pending"}
                            ]
                        },
                        "session_id": sid
                    }));
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "thinking_chunk",
                        "turn_id": turn_id,
                        "text": "Considering the task..."
                    }));
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "thinking_chunk",
                        "turn_id": turn_id,
                        "text": "  - step 1: inspect target file"
                    }));
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "message_chunk",
                        "turn_id": turn_id,
                        "text": "Reading the file now.\n"
                    }));
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "message_chunk",
                        "turn_id": turn_id,
                        "text": "I will request approval before writing.\n"
                    }));
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "tool_call_started",
                        "turn_id": turn_id,
                        "tool_call": {
                            "id": "tc-read-1",
                            "name": "read_file",
                            "arguments": {"path": "/tmp/example.txt"},
                            "kind": "read"
                        }
                    }));
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "tool_call_finished",
                        "tool_call_id": "tc-read-1",
                        "turn_id": turn_id,
                        "status": "succeeded"
                    }));
                    let mut cancelled = false;
                    let approved = if no_approval { true } else {
                    let _ = emit_notification(&mut stdout, "session/update", json!({
                        "kind": "approval_requested",
                        "tool_call_id": "tc-write-1",
                        "turn_id": turn_id,
                        "reason": "write_file changes user content"
                    }));
                    // CRITICAL: flush before blocking on stdin so the host
                    // can read all the notifications emitted above.
                    stdout.flush()?;
                    // Block until the host calls session/approve.
                    loop {
                        let next = match read_frame(&mut locked) {
                            Ok(Some(r)) => r,
                            _ => break false,
                        };
                        let m = next.get("method").and_then(Value::as_str).unwrap_or("");
                        let p = next.get("params").cloned().unwrap_or(Value::Null);
                        let aid = next.get("id").and_then(Value::as_i64);
                        if m == "session/cancel" { cancelled = true; if aid.is_some() { write_response(&mut stdout, aid, json!({"ok":true}))?; } break false; }
                        if m == "session/approve" {
                            let v = p.get("approved").and_then(Value::as_bool).unwrap_or(false);
                            // Per JSON-RPC 2.0, notifications (no `id`) MUST
                            // NOT receive a response. Ridge sends approve as
                            // a notification; only echo when caller used a
                            // request (e.g. older flow).
                            if let Some(aid) = aid {
                                write_response(&mut stdout, Some(aid), json!({ "ok": v }))?;
                                stdout.flush()?;
                            }
                            break v;
                        } else {
                            // Respond with an error so callers don't hang.
                            write_response(
                                &mut stdout,
                                aid,
                                Value::Null,
                            )?;
                            stdout.flush()?;
                        }
                    }
                    };
                    if !approved {
                        let _ = emit_notification(&mut stdout, "session/update", json!({
                            "kind": "turn_failed",
                            "turn_id": turn_id,
                            "error": {"code": -32000, "message": if cancelled { "cancelled" } else { "approval denied" }}
                        }));
                    } else {
                        let _ = emit_notification(&mut stdout, "session/update", json!({
                            "kind": "tool_call_started",
                            "turn_id": turn_id,
                            "tool_call": {
                                "id": "tc-write-1",
                                "name": "write_file",
                                "arguments": {"path": "/tmp/example.txt", "content": "patched"},
                                "kind": "write"
                            }
                        }));
                        let _ = emit_notification(&mut stdout, "session/update", json!({
                            "kind": "tool_call_finished",
                            "tool_call_id": "tc-write-1",
                            "turn_id": turn_id,
                            "status": "succeeded"
                        }));
                        let _ = emit_notification(&mut stdout, "session/update", json!({
                            "kind": "turn_completed",
                            "turn_id": turn_id,
                            "status": "complete"
                        }));
                    }
                    stdout.flush()?;
                } else {
                    write_error(&mut stdout, id, -32000, "no session")?;
                }
            }
            "session/cancel" => {
                if let Some(aid) = id {
                    write_response(&mut stdout, Some(aid), json!({ "ok": true }))?;
                }
                let _ = emit_notification(&mut stdout, "session/update", json!({
                    "kind": "turn_failed",
                    "turn_id": params.get("turn_id").and_then(Value::as_str).unwrap_or(""),
                    "error": {"code": -32001, "message": "cancelled"}
                }));
            }
            "session/approve" => {
                // Without an active prompt, we acknowledge only if a request
                // id was supplied. Notifications (no `id`) MUST NOT receive
                // a response per JSON-RPC 2.0.
                if let Some(aid) = id {
                    let v = params.get("approved").and_then(Value::as_bool).unwrap_or(false);
                    write_response(&mut stdout, Some(aid), json!({ "ok": v }))?;
                }
            }
            _ => {
                write_error(&mut stdout, id, -32601, format!("method not found: {method}"))?;
            }
        }
        stdout.flush()?;
    }
}

fn read_frame<R: BufRead>(r: &mut R) -> io::Result<Option<Value>> {
    let mut header = String::new();
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            if header.is_empty() {
                return Ok(None);
            }
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
    let mut resp = Map::new();
    resp.insert("jsonrpc".into(), Value::String("2.0".into()));
    if let Some(i) = id {
        resp.insert("id".into(), Value::Number(i.into()));
    } else {
        resp.insert("id".into(), Value::Null);
    }
    resp.insert("result".into(), result);
    let body = serde_json::to_vec(&Value::Object(resp)).unwrap();
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)
}

fn write_error<W: Write>(w: &mut W, id: Option<i64>, code: i64, msg: impl Into<String>) -> io::Result<()> {
    let mut resp = Map::new();
    resp.insert("jsonrpc".into(), Value::String("2.0".into()));
    if let Some(i) = id {
        resp.insert("id".into(), Value::Number(i.into()));
    } else {
        resp.insert("id".into(), Value::Null);
    }
    resp.insert(
        "error".into(),
        json!({"code": code, "message": msg.into()}),
    );
    let body = serde_json::to_vec(&Value::Object(resp)).unwrap();
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)
}

fn emit_notification<W: Write>(w: &mut W, method: &str, params: Value) -> io::Result<()> {
    let mut notif = Map::new();
    notif.insert("jsonrpc".into(), Value::String("2.0".into()));
    notif.insert("method".into(), Value::String(method.into()));
    notif.insert("params".into(), params);
    let body = serde_json::to_vec(&Value::Object(notif)).unwrap();
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}
