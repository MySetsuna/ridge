//! Conformance tests for L2-PROTO-001 §3.7 chunked replay resume by
//! since_output_seq, and L2-REMOTE-001 §3.5.1 reconnect semantics.
//!
//! Tests use PtyOutputHub directly (no real PTY child process required):
//! they validate the wire contract that drives `replay_data` framing on the
//! wire and the `since_output_seq` cursor on attach.

use std::sync::Arc;
use std::time::Duration;

use ridge_kernel::pty::{PtyOutputHub, PtyOutputLeaseError, PtyOutputRead};

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
fn attach_with_since_seq_resumes_after_given_cursor() {
    let h = hub();
    h.publish(b"frame-1");
    h.publish(b"frame-2");
    h.publish(b"frame-3");
    // Attach cursor pinned at oldest (None).
    let full = h.attach_output_for_test(None).expect("attach full");
    let r = rt();
    let drained = r.block_on(full.next(Duration::from_millis(50), 16));
    let frames = match drained {
        Ok(PtyOutputRead::Data(f)) => f,
        other => panic!("expected Data, got {other:?}"),
    };
    assert!(frames.len() >= 3, "must drain all 3 frames");
    let cursor_after_full = frames.last().unwrap().seq + 1;

    // New controller attaches with since_output_seq = cursor_after_full:
    // first read must yield Data with first.seq > cursor_after_full, or
    // TimedOut if nothing new has been published since.
    let new_lease = h
        .attach_output_for_test(Some(cursor_after_full))
        .expect("attach resume");
    let read = r.block_on(new_lease.next(Duration::from_millis(50), 16));
    match read {
        Ok(PtyOutputRead::Data(frames)) => {
            assert!(!frames.is_empty());
            assert!(
                frames[0].seq >= cursor_after_full,
                "resumed frames must start at or after since_output_seq"
            );
        }
        Err(PtyOutputLeaseError::TimedOut) => {
            // Acceptable: nothing published yet; contract is that resume
            // does not return stale frames before since_output_seq.
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn replay_does_not_yield_frames_before_since_seq() {
    let h = hub();
    h.publish(b"older-A");
    h.publish(b"older-B");
    h.publish(b"older-C");
    let ahead = h.attach_output_for_test(None).expect("ahead lease");
    let r = rt();
    let drained = r.block_on(ahead.next(Duration::from_millis(50), 16));
    let ahead_frames = match drained {
        Ok(PtyOutputRead::Data(f)) => f,
        other => panic!("expected Data, got {other:?}"),
    };
    let cutoff = ahead_frames.last().unwrap().seq;

    // Laggard attach: since_output_seq = cutoff + 5, older than newest by 4.
    let laggard = h
        .attach_output_for_test(Some(cutoff + 5))
        .expect("laggard lease");
    let read = r.block_on(laggard.next(Duration::from_millis(50), 16));
    match read {
        Ok(PtyOutputRead::Data(frames)) => {
            for f in &frames {
                assert!(
                    f.seq > cutoff + 5,
                    "frame {} must be > since_output_seq={}",
                    f.seq,
                    cutoff + 5
                );
            }
        }
        Err(PtyOutputLeaseError::TimedOut) => {} // also fine; nothing past
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn resync_after_reconnect_resets_cursor_to_oldest() {
    let h = hub();
    h.publish(b"alpha");
    let lease = h.attach_output_for_test(None).expect("attach");
    let r = rt();
    let _ = r.block_on(lease.next(Duration::from_millis(50), 16));

    // After resync, cursor = oldest_seq (= 1, since no eviction yet).
    let cursor = lease.resync().expect("resync ok");
    assert_eq!(cursor, 1, "resync returns oldest_seq");
    let drained = r.block_on(lease.next(Duration::from_millis(50), 16));
    let frames = match drained {
        Ok(PtyOutputRead::Data(f)) => f,
        other => panic!("expected Data, got {other:?}"),
    };
    assert!(!frames.is_empty());
    assert!(frames[0].seq >= 1);
}
