//! Phase A.6 live waterfall measurement: kernel-backed PTY data plane.
//!
//! Connects to a live ridge-kernel HTTP API, spawns an interactive
//! cmd.exe PTY, and measures the round-trip latency from
//! `write_domain_pty` to observed `output` via `poll_domain_pty_output`.
//! Reports waterfall numbers for the input→output path used by the
//! Desktop shell reader.
//!
//! Run with:
//!   cargo test -p ridge-kernel --test kernel_backend_waterfall \
//!     -- --ignored --nocapture
//!
//! Requires a running ridge-kernel on 127.0.0.1:58663 (or set
//! RIDGE_KERNEL_PORT). Skips automatically if no kernel is reachable.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Once;
use std::time::{Duration, Instant};

use ridge_kernel::client::{
    attach_domain_pty_output, list_domain_ptys, poll_domain_pty_output, request_json,
    KernelPtyOutput, KERNEL_REQUEST_TIMEOUT_MS,
};
use ridge_kernel::registry::{read_endpoint, KernelEndpoint};
use uuid::Uuid;

fn http_get(endpoint: &KernelEndpoint, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port))
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(KERNEL_REQUEST_TIMEOUT_MS)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(KERNEL_REQUEST_TIMEOUT_MS)))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\nx-ridge-kernel-token: {}\r\n\r\n",
        endpoint.port, endpoint.token
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut s = String::new();
    stream.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

fn kernel_reachable(endpoint: &KernelEndpoint) -> bool {
    match http_get(endpoint, "/v1/health") {
        Ok(body) => body.contains("\"ok\":true"),
        Err(_) => false,
    }
}

fn spawn_cmd_keep(endpoint: &KernelEndpoint, workspace_id: Uuid, pty_id: Uuid) -> Result<(), String> {
    let body = serde_json::json!({
        "pty_id": pty_id,
        "program": "cmd.exe",
        "args": ["/K"],
        "cwd": "C:\\Windows",
        "workspace_id": workspace_id,
        "role": "shell",
        "cols": 80,
        "rows": 24,
    });
    let _ = request_json(endpoint, "POST", "/v1/domain/ptys", Some(&body))?;
    let _ = list_domain_ptys(endpoint);
    Ok(())
}

fn destroy_pty(endpoint: &KernelEndpoint, pty_id: Uuid) {
    let _ = request_json(
        endpoint,
        "DELETE",
        &format!("/v1/domain/ptys/{pty_id}"),
        None,
    );
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPH: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push(ALPH[((n >> 6) & 63) as usize] as char);
        out.push(ALPH[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push(ALPH[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

#[test]
#[ignore]
fn waterfall_input_to_observed_output() {
    let Some(endpoint) = read_endpoint() else {
        eprintln!("[skip] no kernel endpoint registered");
        return;
    };
    if !kernel_reachable(&endpoint) {
        eprintln!("[skip] kernel not reachable on port {}", endpoint.port);
        return;
    }
    eprintln!("[kernel] port={} pid={}", endpoint.port, endpoint.pid);

    let workspace_id = Uuid::new_v4();
    let pty_id = Uuid::new_v4();
    if let Err(e) = spawn_cmd_keep(&endpoint, workspace_id, pty_id) {
        eprintln!("[skip] spawn failed: {e}");
        return;
    }
    let lease = match attach_domain_pty_output(&endpoint, pty_id, None) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("[skip] attach lease failed: {e}");
            destroy_pty(&endpoint, pty_id);
            return;
        }
    };
    static CLEANUP: Once = Once::new();
    let cleanup = || {
        CLEANUP.call_once(|| {});
        destroy_pty(&endpoint, pty_id);
    };

    // drain initial banner
    std::thread::sleep(Duration::from_millis(300));
    let _ = poll_domain_pty_output(&endpoint, pty_id, lease, 200, 64);

    let iterations = 30usize;
    let mut write_us: Vec<u128> = Vec::with_capacity(iterations);
    let mut first_data_us: Vec<u128> = Vec::with_capacity(iterations);
    let mut rt_us: Vec<u128> = Vec::with_capacity(iterations);
    let mut polls_per_iter: Vec<u32> = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let marker = format!("WM{}\r\n", i);
        let body = serde_json::json!({
            "data_b64": base64_encode(marker.as_bytes()),
        });
        let t_w0 = Instant::now();
        let wr = request_json(
            &endpoint,
            "POST",
            &format!("/v1/domain/ptys/{pty_id}/write"),
            Some(&body),
        );
        let t_w1 = Instant::now();
        write_us.push(t_w1.duration_since(t_w0).as_micros() as u128);
        if wr.is_err() {
            continue;
        }

        let t_p0 = Instant::now();
        let deadline = t_p0 + Duration::from_secs(5);
        let mut polls = 0u32;
        let mut first_data: Option<u128> = None;
        let mut seen_marker = false;
        while Instant::now() < deadline && !seen_marker {
            polls += 1;
            let t_n = Instant::now();
            let res = poll_domain_pty_output(&endpoint, pty_id, lease, 100, 64);
            match res {
                Ok(KernelPtyOutput::Data(buf)) => {
                    if first_data.is_none() && !buf.is_empty() {
                        first_data = Some(t_n.duration_since(t_p0).as_micros() as u128);
                    }
                    if buf.windows(marker.len()).any(|w| w == marker.as_bytes()) {
                        seen_marker = true;
                    }
                }
                Ok(KernelPtyOutput::Timeout) => continue,
                Ok(KernelPtyOutput::Lagged) => break,
                Err(_) => break,
            }
        }
        polls_per_iter.push(polls);
        rt_us.push(t_p0.elapsed().as_micros() as u128);
        if let Some(us) = first_data {
            first_data_us.push(us);
        }
    }

    let pct = |v: &[u128], q: f64| -> u128 {
        if v.is_empty() {
            return 0;
        }
        let idx = ((v.len() as f64 - 1.0) * q).round() as usize;
        v[idx.min(v.len() - 1)]
    };
    let mut sw = write_us.clone();
    sw.sort_unstable();
    let mut sp = first_data_us.clone();
    sp.sort_unstable();
    let mut sr = rt_us.clone();
    sr.sort_unstable();

    eprintln!("[waterfall] iterations={}", iterations);
    eprintln!(
        "[waterfall] input->kernel write_us   P50={}us P95={}us (n={})",
        pct(&sw, 0.50),
        pct(&sw, 0.95),
        sw.len()
    );
    eprintln!(
        "[waterfall] poll->first_data_us    P50={}us P95={}us (n={})",
        pct(&sp, 0.50),
        pct(&sp, 0.95),
        sp.len()
    );
    eprintln!(
        "[waterfall] total input->marker_us P50={}us P95={}us (n={})",
        pct(&sr, 0.50),
        pct(&sr, 0.95),
        sr.len()
    );
    let avg_polls = polls_per_iter.iter().sum::<u32>() as f32 / polls_per_iter.len() as f32;
    eprintln!("[waterfall] avg polls/iter = {:.2}", avg_polls);

    cleanup();
}
