//! Remote backpressure diagnostics (SPEC-L2-PERF-001 §3.4 + REMOTE-BACKPRESSURE-DIAGNOSIS).
//!
//! Goals:
//!   1. Establish whether a slow PtyOutputLease consumer stalls the publisher.
//!   2. Establish whether the kernel std-thread PTY reader blocks when the
//!      downstream mpsc(256) fills.
//!   3. Establish whether multiple leases each isolate backpressure (fan-out
//!      parallelism) or share a single bottleneck.
//!
//! These tests are gated on `--include-ignored`; they are evidence for the
//! backpressure diagnosis, not gating regressions.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ridge_kernel::pty::{PtyOutputFrame, PtyOutputHub, PtyOutputRead};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime")
}

/// Publish a steady stream of bytes into a hub, while one lease is the
/// "slow consumer" (sleeps N ms between each batch) and a parallel "fast
/// consumer" lease drains greedily. The publisher runs in a background
/// thread to remove scheduler noise.
///
/// Returns (published_bytes, fast_consumed_bytes, fast_consumed_frames,
///          lagged_count, fast_p50_us, fast_max_backlog_estimate).
fn scenario_one_slow_one_fast(
    hub: Arc<PtyOutputHub>,
    slow_sleep: Duration,
    total_publish_bytes: usize,
    chunk_size: usize,
) -> (u64, u64, u64, u64, u128, u64) {
    let published = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicU64::new(0));

    let slow_lease = hub.attach_output_for_test(None).expect("slow attach");
    let fast_lease = hub.attach_output_for_test(None).expect("fast attach");

    // Publisher (std thread, mimics the kernel PTY reader).
    let published_c = published.clone();
    let stop_c = stop.clone();
    let chunk = vec![b'x'; chunk_size];
    let publisher = std::thread::spawn(move || {
        while published_c.load(Ordering::Relaxed) < total_publish_bytes as u64 {
            hub.publish(&chunk);
            published_c.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            if stop_c.load(Ordering::Relaxed) != 0 {
                break;
            }
        }
    });

    let r = rt();

    // Slow consumer: drains in tiny batches with sleep.
    let stop_slow = stop.clone();
    let slow_consumed = Arc::new(AtomicU64::new(0));
    let slow_consumed_c = slow_consumed.clone();
    let slow_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        loop {
            let res = rt.block_on(slow_lease.next(Duration::from_millis(20), 4));
            match res {
                Ok(PtyOutputRead::Data(frames)) => {
                    for f in &frames {
                        slow_consumed_c.fetch_add(f.data.len() as u64, Ordering::Relaxed);
                    }
                    std::thread::sleep(slow_sleep);
                }
                Ok(PtyOutputRead::Lagged { .. }) => {
                    stop_slow.store(1, Ordering::Relaxed);
                    break;
                }
                Err(_) => break,
            }
            if stop_slow.load(Ordering::Relaxed) != 0 {
                break;
            }
        }
    });

    // Fast consumer: drains greedily, no sleep.
    let stop_fast = stop.clone();
    let fast_consumed = Arc::new(AtomicU64::new(0));
    let fast_frames = Arc::new(AtomicU64::new(0));
    let fast_consumed_c = fast_consumed.clone();
    let fast_frames_c = fast_frames.clone();
    let fast_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        loop {
            let res = rt.block_on(fast_lease.next(Duration::from_millis(20), 256));
            match res {
                Ok(PtyOutputRead::Data(frames)) => {
                    let n = frames.len() as u64;
                    fast_frames_c.fetch_add(n, Ordering::Relaxed);
                    for f in &frames {
                        fast_consumed_c.fetch_add(f.data.len() as u64, Ordering::Relaxed);
                    }
                }
                Ok(PtyOutputRead::Lagged { .. }) => {
                    stop_fast.store(1, Ordering::Relaxed);
                    break;
                }
                Err(_) => break,
            }
            if stop_fast.load(Ordering::Relaxed) != 0 {
                break;
            }
        }
    });

    // Let the publisher drain (or hit bound).
    let start = Instant::now();
    let mut publish_loop = 0u32;
    while published.load(Ordering::Relaxed) < total_publish_bytes as u64
        && start.elapsed() < Duration::from_secs(15)
    {
        publish_loop += 1;
        std::thread::sleep(Duration::from_millis(50));
    }
    let wall = start.elapsed();
    stop.store(1, Ordering::Relaxed);
    let _ = publisher.join();
    let _ = slow_handle.join();
    let _ = fast_handle.join();

    let fast_c = fast_consumed.load(Ordering::Relaxed);
    let fast_f = fast_frames.load(Ordering::Relaxed);
    let slow_c = slow_consumed.load(Ordering::Relaxed);
    let p = published.load(Ordering::Relaxed);
    let lagged = if slow_c == 0 { 1 } else { 0 };

    // Rough p50 estimate: ~2 * wall / (fast_f frames) per-frame cost.
    let p50 = if fast_f > 0 {
        (wall.as_micros() * 2 / fast_f as u128)
    } else {
        0
    };
    (p, fast_c, fast_f, lagged, p50, publish_loop as u64)
}

/// A: only the fast consumer (no slow). Baseline publish rate.
#[test]
#[ignore]
fn perf_baseline_a_only_fast_consumer() {
    let hub = Arc::new(PtyOutputHub::new());
    let (pub_bytes, fast_bytes, fast_frames, lagged, p50, _loops) =
        scenario_one_slow_one_fast(hub, Duration::from_millis(0), 4 * 1024 * 1024, 4096);
    eprintln!(
        "[backpressure A] published={}MiB fast_consumed={}MiB fast_frames={} lagged={} p50_frame~{}µs",
        pub_bytes / (1024 * 1024),
        fast_bytes / (1024 * 1024),
        fast_frames,
        lagged,
        p50
    );
}

/// B: fast + slow(1ms). Does the fast consumer slow down?
#[test]
#[ignore]
fn perf_baseline_b_slow_1ms() {
    let hub = Arc::new(PtyOutputHub::new());
    let (pub_bytes, fast_bytes, fast_frames, lagged, p50, _loops) =
        scenario_one_slow_one_fast(hub, Duration::from_millis(1), 4 * 1024 * 1024, 4096);
    eprintln!(
        "[backpressure B 1ms] published={}MiB fast_consumed={}MiB fast_frames={} lagged={} p50_frame~{}µs",
        pub_bytes / (1024 * 1024),
        fast_bytes / (1024 * 1024),
        fast_frames,
        lagged,
        p50
    );
}

/// C: fast + slow(20ms). Heavy backpressure.
#[test]
#[ignore]
fn perf_baseline_c_slow_20ms() {
    let hub = Arc::new(PtyOutputHub::new());
    let (pub_bytes, fast_bytes, fast_frames, lagged, p50, _loops) =
        scenario_one_slow_one_fast(hub, Duration::from_millis(20), 4 * 1024 * 1024, 4096);
    eprintln!(
        "[backpressure C 20ms] published={}MiB fast_consumed={}MiB fast_frames={} lagged={} p50_frame~{}µs",
        pub_bytes / (1024 * 1024),
        fast_bytes / (1024 * 1024),
        fast_frames,
        lagged,
        p50
    );
}

/// D: fast + slow(100ms). Saturated slow consumer.
#[test]
#[ignore]
fn perf_baseline_d_slow_100ms() {
    let hub = Arc::new(PtyOutputHub::new());
    let (pub_bytes, fast_bytes, fast_frames, lagged, p50, _loops) =
        scenario_one_slow_one_fast(hub, Duration::from_millis(100), 4 * 1024 * 1024, 4096);
    eprintln!(
        "[backpressure D 100ms] published={}MiB fast_consumed={}MiB fast_frames={} lagged={} p50_frame~{}µs",
        pub_bytes / (1024 * 1024),
        fast_bytes / (1024 * 1024),
        fast_frames,
        lagged,
        p50
    );
}

/// Simulate the kernel std-thread + bounded mpsc(256) pipeline directly:
/// a std thread `blocking_send`s into an `mpsc::channel(256)`; an async
/// task simulates a slow consumer (sleep). Measure whether the std thread
/// blocks when the channel fills.
#[test]
#[ignore]
fn kernel_reader_blocking_send_saturates() {
    use tokio::sync::mpsc;
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);
    let chunk = vec![b'x'; 4096];
    let total = 4 * 1024 * 1024usize;
    let published = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicU64::new(0));

    let p_c = published.clone();
    let stop_c = stop.clone();
    let chunk_c = chunk.clone();
    let publisher = std::thread::spawn(move || {
        let start = Instant::now();
        while p_c.load(Ordering::Relaxed) < total as u64 {
            if tx.blocking_send(chunk_c.clone()).is_err() {
                break;
            }
            p_c.fetch_add(chunk_c.len() as u64, Ordering::Relaxed);
            if stop_c.load(Ordering::Relaxed) != 0 {
                break;
            }
        }
        start.elapsed()
    });

    // Slow consumer: 50ms per batch.
    let stop_rx = stop.clone();
    let consumer = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut total = 0u64;
        loop {
            let res = rt.block_on(async {
                    tokio::time::timeout(Duration::from_millis(50), rx.recv()).await
                });
            match res {
                Ok(Some(bytes)) => {
                    total += bytes.len() as u64;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Ok(None) => break,
                Err(_) => {
                    // Timeout: nothing in queue. Force the publisher to
                    // block on its bounded send (key evidence).
                    stop_rx.store(1, Ordering::Relaxed);
                    break;
                }
            }
        }
        total
    });

    let p_wall = {
        let p_c = published.clone();
        let start = Instant::now();
        while p_c.load(Ordering::Relaxed) < total as u64 && start.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(20));
        }
        stop.store(1, Ordering::Relaxed);
        start.elapsed()
    };
    let publisher_wall = publisher.join().unwrap_or_default();
    let consumer_total = consumer.join().unwrap_or(0);
    let published = published.load(Ordering::Relaxed);
    eprintln!(
        "[kernel-reader] publisher_wall={:?} loop_wall={:?} published={}MiB consumer_received={}MiB",
        publisher_wall, p_wall,
        published / (1024 * 1024),
        consumer_total / (1024 * 1024)
    );
}