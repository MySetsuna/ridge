# Iteration 110 - LAN RPC failure queue cleanup

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`LanOutboundTransport::send_json_rpc` appended every request to
`pending_rpc`, but the error branch left failed synchronous requests queued.
Repeated `write_to_pty`, `resize_pane`, or list failures therefore consumed the
bounded queue and surfaced as secondary backpressure errors.

## Delivered

- Remove the exact failed `(method, params)` entry from `pending_rpc` on both
  success and error, so only genuinely in-flight wire work remains queued.
- Preserve the existing queue cap and backpressure counter.
- Add a regression proving a failed `write_to_pty` leaves queue length zero;
  update the cap test to fill the queue explicitly.

## Verification

- `cargo test -p ridge --lib hosts::lan_transport::tests --quiet`: 6 passed.
- No protocol or version change; no publication was made because `v0.1.54`
  consumed today's release allowance.

## Remaining gates

True asynchronous socket wall-clock timeout, exponential retry pause, and
pending-request cancellation on destroyed panes still require the real WS
read/write loop and remain separate gates.
