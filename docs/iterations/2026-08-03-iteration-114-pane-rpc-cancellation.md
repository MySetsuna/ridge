# Iteration 114 - Pane-scoped pending RPC cancellation

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

Destroying a foreign pane cleared the local subscription, but an asynchronous
LAN transport could still retain queued `write_to_pty`/`resize_pane` frames for
that pane. Those stale frames could arrive after PTY teardown and recreate
`Pane not found` noise or consume queue capacity.

## Delivered

- Add a default no-op `cancel_pending_for_pane` hook to the outbound transport
  abstraction.
- Have `OutboundClient::unsubscribe` invoke it under the per-host RPC gate
  before sending the unsubscribe request.
- LAN transport removes only queued write/resize frames matching the destroyed
  pane, preserving other panes' work.
- Add a regression proving two matching requests are removed while another
  pane's request remains.

## Verification

- `cargo test -p ridge --lib hosts:: --quiet`: 59 passed.
- No protocol or version change; no publication was made because `v0.1.54`
  consumed today's release allowance.

## Remaining gates

Real socket wall-clock expiry and process-level cancellation remain separate
gates; this slice closes queued pane-scoped work at the controller boundary.
