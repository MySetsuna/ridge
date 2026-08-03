# Iteration 109 - Outbound RPC gate

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`OutboundClient` serialized Resize, subscribe, and unsubscribe through a gate,
but `write_to_pty` and connection listing bypassed it. Concurrent terminal
input, layout observers, and disconnect/reconnect work could therefore issue
overlapping synchronous transport calls and build a remote pending queue.

## Delivered

- Renamed the per-host gate to `rpc_gate` and routed connect/list, subscribe,
  unsubscribe, terminal input, Resize, reconnect cache reset, and disconnect
  through it.
- Rechecked subscription state while holding the gate before sending input or
  Resize, preventing writes to a pane that was concurrently detached.
- Added a deterministic concurrent transport test asserting the in-flight RPC
  peak is one while preserving all eight input writes.

## Verification

- `cargo test -p ridge --lib hosts::outbound::tests --quiet`: 11 passed.
- No protocol or version change; no publication was made because `v0.1.54`
  consumed today's release allowance.

## Remaining gates

Transport-level wall-clock timeout, queue cancellation for a destroyed pane,
and public/physical Remote evidence remain separate follow-up gates.
