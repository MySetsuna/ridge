//! Phase 2 conformance: KernelBackedHandle wraps PtyRegistry (PTY process
//! authority) + PtyOutputLease (bounded replay) into a single shell-side
//! handle. Tests use the hub-only path (no real PTY child); full lifecycle
//! integration lives in the kernel binary's domain tests (Phase 5).

use std::sync::Arc;
use std::time::Duration;

use ridge_kernel::kernel_backed_handle::{
    from_parts_for_test, ControllerId, OutputCursor,
};
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
fn attach_with_since_seq_advances_only_after_observed_frames() {
    let h = hub();
    h.publish(b"a");
    h.publish(b"b");
    h.publish(b"c");
    // next_seq == 4 after three publishes.  Pin since=3 so cursor lands at 4
    // and the first next() finds no frame past it (TimedOut).
    let lease = h.attach_output_for_test(Some(3)).expect("attach");
    let mut handle = from_parts_for_test(
        uuid::Uuid::nil(),
        ControllerId::new(),
        OutputCursor(4),
        lease,
    );

    let r = rt();
    // No new publish yet; expect TimedOut, cursor must remain at 4.
    let read = r.block_on(handle.next(Duration::from_millis(20), 16));
    assert!(matches!(read, Err(PtyOutputLeaseError::TimedOut)));
    assert_eq!(
        handle.cursor.0,
        4,
        "TimedOut must not advance cursor (no observed frames)"
    );

    // Publish fresh frame; now next() must observe it and cursor must jump.
    h.publish(b"d");
    let read = r.block_on(handle.next(Duration::from_millis(50), 16));
    let frames = match read {
        Ok(PtyOutputRead::Data(f)) => f,
        other => panic!("expected Data, got {other:?}"),
    };
    assert!(!frames.is_empty());
    let last = frames.last().unwrap().seq;
    assert_eq!(handle.cursor.0, last + 1);
}

#[test]
fn resync_resets_cursor_to_oldest_in_handle() {
    let h = hub();
    h.publish(b"first");
    h.publish(b"second");
    let lease = h.attach_output_for_test(None).expect("attach");
    let mut handle = from_parts_for_test(
        uuid::Uuid::nil(),
        ControllerId::new(),
        OutputCursor(0),
        lease,
    );
    let r = rt();
    let _ = r.block_on(handle.next(Duration::from_millis(50), 16));
    let oldest = handle.resync().expect("resync ok");
    assert_eq!(oldest, 1, "oldest_seq = 1 (no eviction yet)");
    assert_eq!(handle.cursor.0, oldest);
}

#[test]
fn controller_id_scopes_identity_but_does_not_carry_sequence() {
    // SPEC-REMOTE-001 §3.4.5: controller_id is identity, not a sequence.
    // Assert no monotonicity expectation by building two ids and confirming
    // they are distinct but unrelated in seq space.
    let a = ControllerId::new();
    let b = ControllerId::new();
    assert_ne!(a.0, b.0);
    // No .seq() method on ControllerId — verify by compile-time absence:
    // the type exposes only `0` (the inner Uuid).
}

#[test]
fn detached_handle_drops_lease_internally() {
    let h = hub();
    h.publish(b"x");
    let lease = h.attach_output_for_test(None).expect("attach");
    let handle = from_parts_for_test(
        uuid::Uuid::nil(),
        ControllerId::new(),
        OutputCursor(0),
        lease,
    );
    let id = handle.lease_id();
    handle.detach().expect("detach");
    // After detach, a fresh attach must allocate a different lease id
    // (the previous slot was released).
    let next = h.attach_output_for_test(None).expect("attach 2");
    assert_ne!(next.id(), id, "detached lease must release its slot");
}
