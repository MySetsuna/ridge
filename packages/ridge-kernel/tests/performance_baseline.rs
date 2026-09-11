//! Performance baseline (SPEC-L2-PERF-001 §3.2 + §3.5).
//!
//! Captures P50/P95/P99 latency and throughput numbers for the
//! kernel-backed data path. Output is printed to stderr for CI.
//!
//! Run only under `--include-ignored`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ridge_kernel::pty::{PtyOutputFrame, PtyOutputHub, PtyOutputRead};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn percentile(samples: &mut Vec<u128>, pct: f64) -> u128 {
    samples.sort_unstable();
    let idx = ((pct / 100.0) * (samples.len() as f64 - 1.0)) as usize;
    samples[idx]
}

#[test]
#[ignore]
fn perf_baseline_pty_output_throughput() {
    // Attach first so we can drain in lockstep with the publisher.
    let hub = Arc::new(PtyOutputHub::new());
    let lease = hub.attach_output_for_test(None).expect("attach");
    let chunk = vec![b'p'; 8 * 1024];
    let total_bytes: usize = 16 * 1024 * 1024;
    let chunks = total_bytes / chunk.len();

    // Publisher task.
    let publisher = {
        let hub = hub.clone();
        let chunk = chunk.clone();
        std::thread::spawn(move || {
            for _ in 0..chunks {
                hub.publish(&chunk);
            }
        })
    };

    let r = rt();
    let start = Instant::now();
    let mut delivered = 0usize;
    let mut polls = 0u32;
    let mut lagged_at: Option<u64> = None;
    while delivered < total_bytes && polls < 4096 {
        polls += 1;
        match r.block_on(lease.next(Duration::from_millis(50), 256)) {
            Ok(PtyOutputRead::Data(frames)) => {
                for f in frames {
                    delivered += f.data.len();
                }
            }
            Ok(PtyOutputRead::Lagged { oldest_seq, latest_seq, .. }) => {
                lagged_at = Some(latest_seq);
                eprintln!(
                    "[perf-baseline] Lagged during drain: oldest={oldest_seq} latest={latest_seq}"
                );
                break;
            }
            Err(_) => break,
        }
    }
    let _ = publisher.join();
    let elapsed = start.elapsed();
    let mibps = if elapsed.as_secs_f64() > 0.0 {
        (delivered as f64) / (1024.0 * 1024.0) / elapsed.as_secs_f64()
    } else {
        0.0
    };
    eprintln!(
        "[perf-baseline] pty_output_throughput: {} bytes in {:?} ({} polls) = {:.2} MiB/s lagged={:?}",
        delivered, elapsed, polls, mibps, lagged_at
    );
}

#[test]
#[ignore]
fn perf_baseline_input_to_output_single_pane() {
    let hub = Arc::new(PtyOutputHub::new());
    let lease = hub.attach_output_for_test(None).expect("attach");
    let r = rt();
    let mut samples: Vec<u128> = Vec::with_capacity(256);
    for _ in 0..256 {
        let start = Instant::now();
        hub.publish(b"x");
        let _ = r.block_on(lease.next(Duration::from_millis(50), 1));
        samples.push(start.elapsed().as_micros());
    }
    let p50 = percentile(&mut samples.clone(), 50.0);
    let mut samples2 = samples.clone();
    let p95 = percentile(&mut samples2, 95.0);
    let mut samples3 = samples.clone();
    let p99 = percentile(&mut samples3, 99.0);
    eprintln!(
        "[perf-baseline] input_to_output_single_pane: p50={}µs p95={}µs p99={}µs (n={})",
        p50, p95, p99, samples.len()
    );
}

#[test]
#[ignore]
fn perf_baseline_multi_pane_throughput() {
    let hub = Arc::new(PtyOutputHub::new());
    let lease = hub.attach_output_for_test(None).expect("attach");
    let publishers = 16;
    let chunk = vec![b'm'; 4 * 1024];
    let total_per_publisher: usize = 4 * 1024 * 1024;
    let total_bytes = total_per_publisher * publishers;
    let mut handles = Vec::new();
    let start = Instant::now();
    for _ in 0..publishers {
        let hub = hub.clone();
        let chunk = chunk.clone();
        let h = std::thread::spawn(move || {
            let n = total_per_publisher / chunk.len();
            for _ in 0..n {
                hub.publish(&chunk);
            }
        });
        handles.push(h);
    }
    for h in handles {
        let _ = h.join();
    }
    let publish_elapsed = start.elapsed();
    eprintln!(
        "[perf-baseline] multi_pane publish: {} bytes from {} threads in {:?}",
        total_bytes, publishers, publish_elapsed
    );
    let r = rt();
    let drain_start = Instant::now();
    let mut delivered = 0usize;
    while delivered < total_bytes {
        match r.block_on(lease.next(Duration::from_millis(50), 256)) {
            Ok(PtyOutputRead::Data(frames)) => {
                for f in frames {
                    delivered += f.data.len();
                }
            }
            _ => break,
        }
    }
    let drain_elapsed = drain_start.elapsed();
    let mibps = (delivered as f64) / (1024.0 * 1024.0) / drain_elapsed.as_secs_f64();
    eprintln!(
        "[perf-baseline] multi_pane drain: {} bytes in {:?} = {:.2} MiB/s",
        delivered, drain_elapsed, mibps
    );
}

#[test]
#[ignore]
fn perf_baseline_rtp1_fan_out_sizes() {
    use ridge_kernel::rtp1_session::Rtp1Session;
    let registry = Arc::new(ridge_kernel::pty::PtyRegistry::default());
    registry.set_runtime_epoch("epoch-perf".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let total_bytes = 4 * 1024 * 1024;
    let chunk = 24 * 1024;
    let frames: Vec<PtyOutputFrame> = (0..(total_bytes / chunk))
        .map(|i| PtyOutputFrame {
            seq: (i + 1) as u64,
            data: vec![b'F'; chunk],
        })
        .collect();
    let start = Instant::now();
    let out = session.build_output_frames("term", &frames);
    let elapsed = start.elapsed();
    let max_payload = out.iter().map(|f| f.payload.len()).max().unwrap_or(0);
    eprintln!(
        "[perf-baseline] rtp1_fan_out: {} input frames → {} RTP1 frames in {:?}; max_payload={} bytes (cap=65536)",
        frames.len(),
        out.len(),
        elapsed,
        max_payload
    );
    assert!(max_payload <= 65536, "realtime frame cap must hold");
}

#[test]
#[ignore]
fn perf_baseline_rtp1_attach_latency() {
    use ridge_kernel::pty::PtyLaunch;
    use ridge_kernel::rtp1::{AttachMode, AttachRequest};
    use ridge_kernel::rtp1_session::Rtp1Session;
    let registry = Arc::new(ridge_kernel::pty::PtyRegistry::default());
    registry.set_runtime_epoch("epoch-attach".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let pty = rt().block_on(async {
        registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "perf",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn")
    });
    let mut samples: Vec<u128> = Vec::with_capacity(1000);
    for i in 0..1000 {
        let req = AttachRequest {
            host_id: "host".into(),
            runtime_epoch: "epoch-attach".into(),
            session_id: "s".into(),
            terminal_id: pty.to_string(),
            controller_id: format!("ctrl-{i}"),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        let start = Instant::now();
        let _ = session.handle_attach(&req);
        samples.push(start.elapsed().as_micros());
    }
    let p50 = percentile(&mut samples.clone(), 50.0);
    let mut samples2 = samples.clone();
    let p95 = percentile(&mut samples2, 95.0);
    let mut samples3 = samples.clone();
    let p99 = percentile(&mut samples3, 99.0);
    eprintln!(
        "[perf-baseline] rtp1_attach_latency: p50={}µs p95={}µs p99={}µs (n={})",
        p50, p95, p99, samples.len()
    );
}
