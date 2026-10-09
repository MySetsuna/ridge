//! `broken_acp_agent` — MOCK_FIXTURE stdio JSON-RPC server for malformed-input
//! and duplicate-response reader tests.
//!
//! Unlike `fake_acp_agent`, this fixture ignores the request method and emits
//! a fixed script of frames based on the first argv:
//!
//! - argv[1] = "malformed_then_ok": emit one malformed Content-Length body,
//!   then a valid response to any incoming request id.
//! - argv[1] = "duplicate_response": emit two `session/update` notifications,
//!   then on receiving any request id, emit two responses (same id) back.
//! - argv[1] = "no_response": drain stdin forever, never write back.
//!
//! Reads still happen via Content-Length framing so the host sees a real
//! streaming boundary.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn main() -> io::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "malformed_then_ok".to_string());
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut locked = stdin.lock();

    let mut observed_request_id: Option<i64> = None;

    loop {
        let req = match read_frame(&mut locked) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(()),
            Err(_) => return Ok(()),
        };
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let id = req.get("id").and_then(Value::as_i64);
        eprintln!("broken_acp_agent: <- {method} (mode={mode})");
        if observed_request_id.is_none() {
            observed_request_id = id;
        }
        let req_id = id.or(observed_request_id);

        match mode.as_str() {
            "malformed_then_ok" => {
                // One malformed JSON body, then one valid response.
                if !MALFORMED_SENT.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    let bytes = b"this is not valid json {";
                    write_raw(&mut stdout, bytes.len(), bytes)?;
                    stdout.flush()?;
                }
                write_response(&mut stdout, req_id, json!({"ok": true}))?;
                stdout.flush()?;
            }
            "duplicate_response" => {
                write_response(&mut stdout, req_id, json!({"first": true}))?;
                write_response(&mut stdout, req_id, json!({"second": true}))?;
                stdout.flush()?;
            }
            "no_response" => {
                // Stay silent — host must rely on shutdown / EOF detection.
                continue;
            }
            _ => {
                write_response(&mut stdout, req_id, json!({"unknown_mode": mode}))?;
                stdout.flush()?;
            }
        }
    }
}

use std::sync::atomic::AtomicBool;
static MALFORMED_SENT: AtomicBool = AtomicBool::new(false);

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
    let body = serde_json::to_vec(&resp).unwrap();
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)
}

fn write_raw<W: Write>(w: &mut W, len: usize, bytes: &[u8]) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n", len)?;
    w.write_all(bytes)
}
