# Iteration 112 - Kernel-first host disconnect

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`disconnect_host_outbound` tore down the live transport and buffers before
writing `HostStatus::Disconnected` through the legacy best-effort mirror. If
the kernel was unavailable, the UI lost its retryable client while the durable
topology still said Connected.

## Delivered

- Add a kernel-authoritative status transition and make the user-visible
  disconnect command commit it before transport teardown.
- On kernel failure, return the error with the outbound client, subscriptions,
  buffers, and Connected projection intact for retry.
- Keep the local projection callback explicit in tests; production has no
  shell-only fallback.
- Add a regression for fail-closed disconnect behavior.

## Verification

- `cargo test -p ridge --lib hosts::tests --quiet`: 20 passed.
- No protocol or version change; no publication was made because `v0.1.54`
  consumed today's release allowance.

## Remaining gates

Combined session/status transactions and reconnect success/failure projection
still need kernel lifecycle ownership and cross-process evidence.
