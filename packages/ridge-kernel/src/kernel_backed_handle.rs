//! KernelBackedHandle: a thin wrapper that ties together PtyRegistry
//! (PTY process authority) + PtyOutputLease (bounded output replay).
//!
//! Phase 2 first step: define the type shell-side code should hold once the
//! PtyHandle dual-source migration lands. Today shell PtyHandle retains
//! `master`/`writer`/`_child` directly; tomorrow it must hold only this
//! handle + a kernel client handle. This module is the seam: it touches no
//! src-tauri internals so we can unit-test it without Tauri.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::pty::{PtyOutputLease, PtyOutputRead, PtyRegistry};

/// Client identity (per SPEC-REMOTE-001 §3.4.5): unique within
/// `(host_id, runtime_epoch, terminal_id)`. v1 uses random UUID v4; the v4→v7
/// migration is a Phase 4 follow-up and does not change this API.
#[derive(Debug, Clone)]
pub struct ControllerId(pub Uuid);

impl ControllerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ControllerId {
    fn default() -> Self {
        Self::new()
    }
}

/// Cursor over the per-terminal output stream. Monotonically advances on
/// each successful read; `since_output_seq` drives `replay`/`resync`.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputCursor(pub u64);

/// KernelBackedHandle: shell-side mirror of a kernel-owned PTY.
///
/// Holds:
///   * `pty_id`    — stable identity inside the kernel registry
///   * `controller_id` — per `(runtime_epoch, terminal)` identity
///   * `cursor`    — last seen output_seq (advances on each `next()`)
///   * `lease`     — bounded replay lease handle (Drop-detaches)
///
/// Does NOT own the master fd / writer / child; those live exclusively in
/// `PtyBridge` inside `PtyRegistry`.
pub struct KernelBackedHandle {
    pub pty_id: Uuid,
    pub controller_id: ControllerId,
    pub cursor: OutputCursor,
    pub lease: PtyOutputLease,
}

impl KernelBackedHandle {
    /// Wrap an existing PTY id as a kernel-backed handle. The caller must
    /// have already spawned the PTY via `PtyRegistry` (or via the kernel
    /// HTTP `domain_pty_create`). The lease attaches from `cursor+1` so the
    /// caller does not see frames it has already consumed.
    pub fn attach(
        registry: &Arc<PtyRegistry>,
        pty_id: Uuid,
        controller_id: ControllerId,
        since_output_seq: Option<u64>,
    ) -> Result<Self, String> {
        let lease = registry
            .attach_output(pty_id, since_output_seq)
            .map_err(|e| format!("attach_output: {e}"))?;
        Ok(Self {
            pty_id,
            controller_id,
            cursor: OutputCursor(since_output_seq.unwrap_or(0)),
            lease,
        })
    }

    pub fn advance(&mut self, frames: &[crate::pty::PtyOutputFrame]) {
        if let Some(last) = frames.last() {
            self.cursor = OutputCursor(last.seq + 1);
        }
    }

    /// Long-poll next batch. On success advances the local cursor.
    pub async fn next(
        &mut self,
        timeout: Duration,
        max_frames: usize,
    ) -> Result<PtyOutputRead, crate::pty::PtyOutputLeaseError> {
        let read = self.lease.next(timeout, max_frames).await?;
        if let PtyOutputRead::Data(ref frames) = read {
            self.advance(frames);
        }
        Ok(read)
    }

    /// Snapshot-only resync: cursor jumps to oldest retained seq.
    pub fn resync(&mut self) -> Result<u64, crate::pty::PtyOutputLeaseError> {
        let oldest = self.lease.resync()?;
        self.cursor = OutputCursor(oldest);
        Ok(oldest)
    }

    pub fn detach(self) -> Result<(), crate::pty::PtyOutputLeaseError> {
        self.lease.detach()
    }

    /// Lease id, exposed for diagnostics. Not part of the wire identity.
    pub fn lease_id(&self) -> Uuid {
        self.lease.id()
    }
}

/// Test-only constructor: bind a handle to a pre-existing lease, bypassing
/// PtyRegistry. Used by integration tests that exercise cursor/resync
/// semantics without spawning a real PTY child process.
#[doc(hidden)]
pub fn from_parts_for_test(
    pty_id: Uuid,
    controller_id: ControllerId,
    cursor: OutputCursor,
    lease: PtyOutputLease,
) -> KernelBackedHandle {
    KernelBackedHandle { pty_id, controller_id, cursor, lease }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_id_is_unique_per_call() {
        let a = ControllerId::new();
        let b = ControllerId::new();
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn output_cursor_default_is_zero() {
        assert_eq!(OutputCursor::default().0, 0);
    }
}
