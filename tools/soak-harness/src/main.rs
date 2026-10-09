//! v0.1.92 closeout soak harness — drives kernel-level scenarios and
//! captures metrics every 2s. Outputs CSV per scenario + a summary JSON.
//!
//! Usage:
//!   soak-harness --scenario host-only --duration 480 --output artifacts/v0.1.92-closeout/host-only
//!
//! Each scenario:
//!   1. Spawn `ridge kernel ensure` (auto-reuses an existing kernel or
//!      brings one up under an isolated RIDGE_KERNEL_DATA_DIR).
//!   2. Poll /v1/metrics every 2s; capture RSS via tasklist.
//!   3. Drive the scenario-specific work loop.
//!   4. On exit, write CSV (`*-metrics.csv`) + summary JSON.
//!
//! The harness focuses on the **kernel** substrate (PTYs, leases,
//! output-hub subscribers, replay-window pressure). Tauri/desktop
//! resources (terminal instances, SurfaceHost, parked terminals, WGPU)
//! need separate instrumentation — those require the desktop app to
//! run, which is out of scope for the local-dev kernel harness.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy)]
enum Scenario {
    HostOnly,
    PlusOnePty,
    PlusMultiPty,
    RapidAttachDetach,
    ReconnectLoop,
    LongScrollback,
    ContinuousOutput,
    BackgroundForeground,
}

impl Scenario {
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "host-only" => Self::HostOnly,
            "plus-one-pty" => Self::PlusOnePty,
            "plus-multi-pty" => Self::PlusMultiPty,
            "rapid-attach-detach" => Self::RapidAttachDetach,
            "reconnect-loop" => Self::ReconnectLoop,
            "long-scrollback" => Self::LongScrollback,
            "continuous-output" => Self::ContinuousOutput,
            "background-foreground" => Self::BackgroundForeground,
            other => return Err(anyhow!("unknown scenario: {other}")),
        })
    }

    fn label(&self) -> &'static str {
        match self {
            Self::HostOnly => "host_only",
            Self::PlusOnePty => "plus_one_pty",
            Self::PlusMultiPty => "plus_multi_pty",
            Self::RapidAttachDetach => "rapid_attach_detach",
            Self::ReconnectLoop => "reconnect_loop",
            Self::LongScrollback => "long_scrollback",
            Self::ContinuousOutput => "continuous_output",
            Self::BackgroundForeground => "background_foreground",
        }
    }
}

struct Args {
    scenario: Scenario,
    duration: Duration,
    output_dir: PathBuf,
    ridge_bin: PathBuf,
}

fn parse_args() -> Result<Args> {
    let mut scenario = None;
    let mut duration_secs: u64 = 480;
    let mut output_dir: Option<PathBuf> = None;
    let mut ridge_bin: Option<PathBuf> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                let v = args.next().context("--scenario needs value")?;
                scenario = Some(Scenario::parse(&v)?);
            }
            "--duration" => {
                let v = args.next().context("--duration needs value")?;
                duration_secs = v.parse().context("--duration must be seconds (integer)")?;
            }
            "--output" => {
                let v = args.next().context("--output needs value")?;
                output_dir = Some(PathBuf::from(v));
            }
            "--ridge-bin" => {
                let v = args.next().context("--ridge-bin needs value")?;
                ridge_bin = Some(PathBuf::from(v));
            }
            other => return Err(anyhow!("unknown flag: {other}")),
        }
    }

    let scenario = scenario.context("--scenario required")?;
    let output_dir = output_dir.unwrap_or_else(|| {
        PathBuf::from(format!("artifacts/v0.1.92-closeout/{}", scenario.label()))
    });
    let ridge_bin = ridge_bin.unwrap_or_else(|| {
        // Default to target/debug/ridge.exe from repo root.
        PathBuf::from("target/debug/ridge.exe")
    });

    Ok(Args {
        scenario,
        duration: Duration::from_secs(duration_secs),
        output_dir,
        ridge_bin,
    })
}

struct KernelHandle {
    data_dir: PathBuf,
    endpoint: Value,
    token: String,
    pid: u32,
}

fn isolated_data_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ridge-soak-{}-{}", std::process::id(), nanos))
}

fn ensure_kernel(bin: &Path, data_dir: &Path) -> Result<KernelHandle> {
    fs::create_dir_all(data_dir).context("create data_dir")?;
    let mut child = Command::new(bin)
        .args(["kernel", "ensure"])
        .env("RIDGE_KERNEL_DATA_DIR", data_dir)
        .env("RIDGE_CONFIRM_QUIT_KERNEL", "1")
        .env("RIDGE_TEST_ALLOW_NON_BREAKAWAY", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn ridge kernel ensure")?;
    let deadline = Instant::now() + Duration::from_secs(25);
    let kernel_json = data_dir.join("kernel.json");
    let endpoint: Value = loop {
        if let Ok(raw) = fs::read_to_string(&kernel_json) {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                if let (Some(port), Some(token), Some(pid)) = (
                    v.get("port").and_then(Value::as_u64),
                    v.get("token").and_then(Value::as_str),
                    v.get("pid").and_then(Value::as_u64),
                ) {
                    if health_ok(port).unwrap_or(false) {
                        break json!({
                            "port": port,
                            "token": token,
                            "pid": pid,
                        });
                    }
                }
            }
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("kernel did not become healthy within 25s"));
        }
        std::thread::sleep(Duration::from_millis(80));
    };
    if child.try_wait()?.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(KernelHandle {
        data_dir: data_dir.to_path_buf(),
        endpoint: endpoint.clone(),
        token: endpoint["token"].as_str().unwrap().to_string(),
        pid: endpoint["pid"].as_u64().unwrap() as u32,
    })
}

fn health_ok(port: u64) -> Result<bool> {
    let url = format!("http://127.0.0.1:{port}/v1/health");
    let resp = ureq::get(&url)
        .timeout(Duration::from_secs(2))
        .call();
    Ok(resp.is_ok())
}

fn request_json(
    port: u64,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let mut req = match method {
        "GET" => ureq::get(&url),
        "POST" => ureq::post(&url),
        "DELETE" => ureq::delete(&url),
        other => return Err(anyhow!("unsupported method: {other}")),
    }
    .set("x-ridge-kernel-token", token)
    .set("Content-Type", "application/json")
    .timeout(Duration::from_secs(10));
    let resp = if let Some(b) = body {
        req.send_json(b.clone())
    } else {
        req.call()
    };
    match resp {
        Ok(r) => Ok(r.into_json()?),
        Err(e) => Err(anyhow!("{method} {path} failed: {e}")),
    }
}

fn fetch_metrics(port: u64, token: &str) -> Result<Value> {
    request_json(port, token, "GET", "/v1/metrics", None)
}

fn fetch_rss(pid: u32) -> Result<u64> {
    // tasklist /FI "PID eq <pid>" /FO CSV /NH → parse Working Set Size (bytes)
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .context("tasklist")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().context("empty tasklist output")?;
    // CSV: handle commas inside quoted fields (e.g. "18,336 K").
    // Walk char-by-char, toggling on `"`.
    let mut fields: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in line.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    if fields.len() < 5 {
        return Err(anyhow!("unexpected tasklist columns: {fields:?}"));
    }
    let mem = &fields[4];
    // Strip non-digits (commas, " K", etc.) — final value is KB working-set.
    let digits: String = mem.chars().filter(|c| c.is_ascii_digit()).collect();
    let kb: u64 = digits.parse().context("parse mem digits")?;
    Ok(kb * 1024)
}

fn create_pty(port: u64, token: &str, cwd: &str, name: &str) -> Result<String> {
    let body = json!({
        "cwd": cwd,
        "shell": "cmd.exe",
        "name": name,
        "cols": 80,
        "rows": 24,
    });
    let v = request_json(port, token, "POST", "/v1/domain/ptys", Some(&body))?;
    let pty_id = v["pty_id"]
        .as_str()
        .context("missing pty_id in response")?
        .to_string();
    Ok(pty_id)
}

fn write_pty(port: u64, token: &str, pty_id: &str, bytes: &[u8]) -> Result<()> {
    use base64::Engine;
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let body = json!({ "data_b64": data_b64 });
    let _ = request_json(
        port,
        token,
        "POST",
        &format!("/v1/domain/ptys/{pty_id}/write"),
        Some(&body),
    )?;
    Ok(())
}

fn attach_lease(port: u64, token: &str, pty_id: &str) -> Result<String> {
    let v = request_json(
        port,
        token,
        "POST",
        &format!("/v1/domain/ptys/{pty_id}/output"),
        None,
    )?;
    let lease_id = v["lease_id"]
        .as_str()
        .context("missing lease_id")?
        .to_string();
    Ok(lease_id)
}

fn detach_lease(port: u64, token: &str, pty_id: &str, lease_id: &str) -> Result<()> {
    let _ = request_json(
        port,
        token,
        "DELETE",
        &format!("/v1/domain/ptys/{pty_id}/output/{lease_id}"),
        None,
    )?;
    Ok(())
}

fn destroy_pty(port: u64, token: &str, pty_id: &str) -> Result<()> {
    let _ = request_json(
        port,
        token,
        "DELETE",
        &format!("/v1/domain/ptys/{pty_id}"),
        None,
    )?;
    Ok(())
}

fn stop_kernel(bin: &Path, data_dir: &Path) -> Result<()> {
    let _ = Command::new(bin)
        .args(["kernel", "stop"])
        .env("RIDGE_KERNEL_DATA_DIR", data_dir)
        .env("RIDGE_CONFIRM_QUIT_KERNEL", "1")
        .output();
    let _ = fs::remove_dir_all(data_dir);
    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let args = parse_args()?;
    fs::create_dir_all(&args.output_dir).context("create output dir")?;

    let data_dir = isolated_data_dir();
    eprintln!("[soak] booting kernel under {}", data_dir.display());
    let handle = ensure_kernel(&args.ridge_bin, &data_dir)?;
    let port = handle.endpoint["port"].as_u64().unwrap();
    eprintln!(
        "[soak] kernel up: pid={} port={}",
        handle.pid, port
    );

    // Set up scenario work loop in a separate task
    let scenario = args.scenario;
    let port_w = port;
    let token_w = handle.token.clone();
    let work_handle = tokio::spawn(async move {
        run_scenario_work(scenario, port_w, &token_w, args.duration).await
    });

    // Poll metrics + RSS every 2s for the full duration
    let metrics_csv = args.output_dir.join("metrics.csv");
    let mut csv = fs::File::create(&metrics_csv).context("create metrics.csv")?;
    // Force line-buffered writes so each poll's row is committed even if the
    // process is killed mid-loop. Plain fs::File batches full buffers which
    // can drop trailing rows.
    use std::io::Write;
    writeln!(
        csv,
        "elapsed_s,wall_unix_ms,pty_count,output_lease_count,subscriber_total,workspace_count,remote_host_count,rss_bytes,kernel_pty_running,kernel_pty_total_subscribers"
    )?;
    csv.flush()?;
    csv.sync_data()?;

    let start = Instant::now();
    let poll_interval = Duration::from_secs(2);
    while start.elapsed() < args.duration {
        let elapsed = start.elapsed().as_secs_f64();
        let wall_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let metrics = fetch_metrics(port, &handle.token).unwrap_or(json!({}));
        let rss = fetch_rss(handle.pid).unwrap_or(0);
        let pty_count = metrics["pty_count"].as_u64().unwrap_or(0);
        let output_lease_count = metrics["output_lease_count"].as_u64().unwrap_or(0);
        let subscriber_total = metrics["group_subscription_total"].as_u64().unwrap_or(0);
        let workspace_count = metrics["workspace_count"].as_u64().unwrap_or(0);
        let remote_host_count = metrics["remote_host_count"].as_u64().unwrap_or(0);
        let kernel_pty_running = metrics["ptys"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|p| p["lifecycle"] == "Running")
                    .count()
            })
            .unwrap_or(0);
        let kernel_pty_total_subscribers = metrics["ptys"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["subscriber_count"].as_u64())
                    .sum::<u64>()
            })
            .unwrap_or(0);
        writeln!(
            csv,
            "{:.2},{wall_unix},{pty_count},{output_lease_count},{subscriber_total},{workspace_count},{remote_host_count},{rss},{kernel_pty_running},{kernel_pty_total_subscribers}",
            elapsed
        )?;
        csv.flush()?;
        csv.sync_data()?;
        eprintln!(
            "[soak] t={:>5.1}s  ptys={} leases={} subs={} ws={} rss={}KB",
            elapsed, pty_count, output_lease_count, subscriber_total, workspace_count, rss / 1024
        );
        tokio::time::sleep(poll_interval).await;
    }

    // Signal scenario work to stop (via work_handle drop on exit)
    let _ = work_handle.await;

    // Final metrics snapshot
    let final_metrics = fetch_metrics(port, &handle.token).unwrap_or(json!({}));
    let summary_path = args.output_dir.join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&json!({
            "scenario": args.scenario.label(),
            "duration_s": args.duration.as_secs(),
            "final_metrics": final_metrics,
            "final_rss_bytes": fetch_rss(handle.pid).unwrap_or(0),
        }))?,
    )?;

    // Stop kernel + cleanup
    stop_kernel(&args.ridge_bin, &data_dir)?;
    eprintln!("[soak] done; outputs in {}", args.output_dir.display());
    Ok(())
}

async fn run_scenario_work(scenario: Scenario, port: u64, token: &str, duration: Duration) -> Result<()> {
    let stop_at = Instant::now() + duration;
    match scenario {
        Scenario::HostOnly => {
            // Pure baseline: kernel running, no PTYs.
            tokio::time::sleep(duration).await;
        }
        Scenario::PlusOnePty => {
            let pty = create_pty(port, token, ".", "soak-plus-one")?;
            eprintln!("[soak] created pty {pty}");
            // Continuous small output
            while Instant::now() < stop_at {
                write_pty(port, token, &pty, b"echo RIDGE_SOAK_PLUS_ONE\n")?;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            destroy_pty(port, token, &pty).ok();
        }
        Scenario::PlusMultiPty => {
            // 8 PTYs, each with output
            let ptys: Vec<String> = (0..8)
                .map(|i| create_pty(port, token, ".", &format!("soak-multi-{i}")))
                .collect::<Result<_>>()?;
            while Instant::now() < stop_at {
                for p in &ptys {
                    let _ = write_pty(port, token, p, b"echo RIDGE_SOAK_MULTI\n");
                }
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
            for p in ptys {
                destroy_pty(port, token, &p).ok();
            }
        }
        Scenario::RapidAttachDetach => {
            // 4 PTYs; rapid attach/detach lease cycles
            let ptys: Vec<String> = (0..4)
                .map(|i| create_pty(port, token, ".", &format!("soak-rapid-{i}")))
                .collect::<Result<_>>()?;
            while Instant::now() < stop_at {
                for p in &ptys {
                    let lease = attach_lease(port, token, p)?;
                    // Read 1 batch then detach
                    let _ = request_json(
                        port,
                        token,
                        "GET",
                        &format!("/v1/domain/ptys/{p}/output/{lease}?timeout_ms=50&max_frames=8"),
                        None,
                    );
                    detach_lease(port, token, p, &lease).ok();
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            for p in ptys {
                destroy_pty(port, token, &p).ok();
            }
        }
        Scenario::ReconnectLoop => {
            // 1 PTY; attach lease, write, detach, repeat
            let pty = create_pty(port, token, ".", "soak-reconnect")?;
            while Instant::now() < stop_at {
                let lease = attach_lease(port, token, &pty)?;
                write_pty(port, token, &pty, b"echo RIDGE_SOAK_RECONNECT\n")?;
                let _ = request_json(
                    port,
                    token,
                    "GET",
                    &format!("/v1/domain/ptys/{pty}/output/{lease}?timeout_ms=50&max_frames=16"),
                    None,
                );
                detach_lease(port, token, &pty, &lease).ok();
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            destroy_pty(port, token, &pty).ok();
        }
        Scenario::LongScrollback => {
            // 1 PTY; dump ~80 MB of output via `yes`
            let pty = create_pty(port, token, ".", "soak-long-scrollback")?;
            // Stream output by writing big payload directly (no shell needed)
            let chunk: Vec<u8> = (b'A'..=b'Z').cycle().take(8192).collect();
            let target_bytes: u64 = 80 * 1024 * 1024;
            let mut written: u64 = 0;
            while written < target_bytes && Instant::now() < stop_at {
                write_pty(port, token, &pty, &chunk)?;
                written += chunk.len() as u64;
                // No sleep — saturate
            }
            destroy_pty(port, token, &pty).ok();
        }
        Scenario::ContinuousOutput => {
            // 1 PTY; ~4 KB/s sustained output for full duration
            let pty = create_pty(port, token, ".", "soak-continuous")?;
            let chunk: Vec<u8> = b"0123456789ABCDEF\n".to_vec(); // 17 bytes
            let chunks_per_sec = 235; // 17 * 235 ≈ 4 KB/s
            let interval = Duration::from_micros(1_000_000 / chunks_per_sec);
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut sent: u64 = 0;
            while Instant::now() < stop_at {
                ticker.tick().await;
                write_pty(port, token, &pty, &chunk).ok();
                sent += chunk.len() as u64;
            }
            eprintln!("[soak] continuous sent {} bytes", sent);
            destroy_pty(port, token, &pty).ok();
        }
        Scenario::BackgroundForeground => {
            // No kernel-side visibility for tab visibility; just attach a
            // lease, read 0 bytes, detach, repeat with varying intervals
            let pty = create_pty(port, token, ".", "soak-bgfg")?;
            let lease = attach_lease(port, token, &pty)?;
            let mut phase = 0u32;
            while Instant::now() < stop_at {
                // Background phase: no reads
                tokio::time::sleep(Duration::from_secs(30)).await;
                // Foreground phase: burst reads
                for _ in 0..50 {
                    let _ = request_json(
                        port,
                        token,
                        "GET",
                        &format!("/v1/domain/ptys/{pty}/output/{lease}?timeout_ms=10&max_frames=8"),
                        None,
                    );
                }
                phase += 1;
                eprintln!("[soak] bgfg phase {phase}");
            }
            detach_lease(port, token, &pty, &lease).ok();
            destroy_pty(port, token, &pty).ok();
        }
    }
    Ok(())
}
