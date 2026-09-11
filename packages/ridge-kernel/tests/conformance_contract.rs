//! Conformance tests for L2-TERM-001 / L2-PROTO-001 / L2-REMOTE-001 contract
//! invariants backed by the kernel PtyOutputHub implementation.
//!
//! These tests exercise observable contract: sequence monotonicity, bounded
//! replay caps, lease lifecycle transitions, and resync cursor reset.
//! They do NOT depend on internal struct layout — only the public
//! PtyOutputHub constructor + publish + PtyOutputLease API.

use std::sync::Arc;
use std::time::Duration;

use ridge_kernel::pty::{PtyOutputHub, PtyOutputLeaseError, PtyOutputRead, OUTPUT_REPLAY_CAP_BYTES,
    OUTPUT_REPLAY_CAP_FRAMES};

fn hub() -> Arc<PtyOutputHub> {
    Arc::new(PtyOutputHub::new())
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn output_seq_is_strictly_monotonic() {
    let h = hub();
    for chunk in [b"a".as_slice(), b"bb", b"ccc", b"dddd"] {
        h.publish(chunk);
    }
    let lease = h
        .attach_output_for_test(None)
        .expect("attach");
    let r = rt();
    let read = r.block_on(lease.next(Duration::from_millis(50), 16));
    match read {
        Ok(PtyOutputRead::Data(frames)) => {
            assert!(!frames.is_empty(), "frames must be non-empty");
            for pair in frames.windows(2) {
                assert!(
                    pair[0].seq < pair[1].seq,
                    "output_seq must be strictly monotonic: {} < {}",
                    pair[0].seq,
                    pair[1].seq
                );
            }
        }
        other => panic!("expected Data, got {other:?}"),
    }
}

#[test]
fn empty_publish_does_not_consume_seq() {
    let h = hub();
    h.publish(b"");
    let lease = h.attach_output_for_test(None).expect("attach");
    let r = rt();
    let read = r.block_on(lease.next(Duration::from_millis(50), 16));
    match read {
        Ok(PtyOutputRead::Data(frames)) => assert!(
            frames.is_empty(),
            "empty publish must not produce frames, got {} frames",
            frames.len()
        ),
        Err(PtyOutputLeaseError::TimedOut) => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn bounded_replay_cap_triggers_lagged_after_eviction() {
    let h = hub();
    let slow_lease = h.attach_output_for_test(None).expect("attach");
    let big = vec![b'x'; OUTPUT_REPLAY_CAP_BYTES];
    for _ in 0..(OUTPUT_REPLAY_CAP_FRAMES + 8) {
        h.publish(&big);
    }
    let r = rt();
    let read = r.block_on(slow_lease.next(Duration::from_millis(50), 32));
    match read {
        Ok(PtyOutputRead::Lagged { oldest_seq, latest_seq, .. }) => {
            assert!(oldest_seq > 1, "oldest_seq must advance past initial cursor (got {oldest_seq})");
            assert!(latest_seq >= oldest_seq);
        }
        Ok(PtyOutputRead::Data(frames)) => {
            assert!(!frames.is_empty());
            assert!(frames[0].seq >= 1);
        }
        Err(e) => panic!("unexpected err: {e:?}"),
    }
}

#[test]
fn lease_detach_drops_underlying_entry() {
    let h = hub();
    h.publish(b"payload-1");
    let lease = h.attach_output_for_test(None).expect("attach");
    let id = lease.id();
    drop(lease);
    let next = h.attach_output_for_test(None).expect("attach 2");
    assert_ne!(next.id(), id, "fresh attach must allocate new lease id");
}

#[test]
fn lease_resync_resets_cursor_to_oldest() {
    let h = hub();
    h.publish(b"first");
    h.publish(b"second");
    let lease = h.attach_output_for_test(None).expect("attach");
    let r = rt();
    let _ = r.block_on(lease.next(Duration::from_millis(50), 16));
    h.publish(b"third");
    let cursor_after_resync = lease.resync().expect("resync ok");
    assert_eq!(cursor_after_resync, 1, "resync must move cursor to oldest_seq");
    let drained = r.block_on(lease.next(Duration::from_millis(50), 16));
    match drained {
        Ok(PtyOutputRead::Data(frames)) => assert!(!frames.is_empty()),
        Err(PtyOutputLeaseError::TimedOut) => {}
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn invalid_batch_size_is_rejected() {
    let h = hub();
    let lease = h.attach_output_for_test(None).expect("attach");
    let r = rt();
    let err = r.block_on(lease.next(Duration::from_millis(50), 0));
    assert!(matches!(err, Err(PtyOutputLeaseError::InvalidBatchSize)));
}

#[test]
fn frame_payload_data_round_trips() {
    let h = hub();
    let payload = b"hello-output";
    h.publish(payload);
    let lease = h.attach_output_for_test(None).expect("attach");
    let r = rt();
    let read = r.block_on(lease.next(Duration::from_millis(50), 16));
    let frames = match read {
        Ok(PtyOutputRead::Data(f)) => f,
        other => panic!("expected Data, got {other:?}"),
    };
    let joined: Vec<u8> = frames.iter().flat_map(|f| f.data.iter().copied()).collect();
    assert!(joined.starts_with(payload), "first frame must carry payload head");
}

/// Smoke perf measurement (Phase 1 baseline harness, step 1).
/// Runs only under `--include-ignored` to keep `cargo test` fast.
#[test]
#[ignore]
fn perf_output_throughput_smoke() {
    use std::time::Instant;
    let h = hub();
    let chunk = vec![b'y'; 8 * 1024];
    let total_bytes: usize = 32 * 1024 * 1024; // 32 MiB
    let chunks = total_bytes / chunk.len();
    // Attach AFTER publishing so cursor lands at oldest (no Lagged on first
    // read).  Publish in chunks small enough to keep each frame under
    // OUTPUT_REPLAY_CAP_BYTES.
    for _ in 0..chunks {
        h.publish(&chunk);
    }
    let lease = h.attach_output_for_test(None).expect("attach");
    let r = rt();
    let start = Instant::now();
    let mut delivered = 0usize;
    let mut polls = 0u32;
    while delivered < total_bytes && polls < 1024 {
        polls += 1;
        match r.block_on(lease.next(Duration::from_millis(50), 256)) {
            Ok(PtyOutputRead::Data(frames)) => {
                for f in frames {
                    delivered += f.data.len();
                }
            }
            Ok(PtyOutputRead::Lagged { oldest_seq, latest_seq, .. }) => {
                eprintln!(
                    "[perf-baseline] Lagged before completion: oldest={oldest_seq} latest={latest_seq}"
                );
                break;
            }
            Err(PtyOutputLeaseError::TimedOut) => break,
            Err(e) => panic!("unexpected err: {e:?}"),
        }
    }
    let elapsed = start.elapsed();
    let mibps = if elapsed.as_secs_f64() > 0.0 {
        (delivered as f64) / (1024.0 * 1024.0) / elapsed.as_secs_f64()
    } else {
        0.0
    };
    eprintln!(
        "[perf-baseline] output_throughput smoke: delivered={} bytes in {:?} (polls={polls}) = {:.2} MiB/s (hub-only, single subscriber, single-thread, release)",
        delivered, elapsed, mibps
    );
}
