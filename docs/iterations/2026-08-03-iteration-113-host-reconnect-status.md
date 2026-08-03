# Iteration 113 - Kernel-authoritative reconnect status

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

Disconnect now durably marks a host `Disconnected`, but a successful outbound
resubscribe only changed the transport/supervisor state. The kernel topology
could therefore remain Disconnected forever and the Hosts UI would show a
stale failure after a healthy reconnect.

## Delivered

- After reconnect success, project `HostStatus::Connected` and detail through
  the kernel-authoritative status writer.
- Retry status projection when the write fails: the supervisor remains in
  `Succeeded` until a later command can commit the durable status, rather than
  collapsing to `Idle` and hiding the failure.
- Apply the same status projection before collapsing an already-successful
  reconnect on the next poll.

## Verification

- `cargo test -p ridge --lib hosts::tests --quiet`: 20 passed.
- No protocol or version change; no publication was made because `v0.1.54`
  consumed today's release allowance.

## Remaining gates

Atomic combined session/status reconnect commits and real cross-process host
reconnect evidence remain open.
