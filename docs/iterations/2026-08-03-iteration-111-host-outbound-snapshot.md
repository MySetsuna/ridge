# Iteration 111 - Kernel-authoritative outbound snapshot

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`bind_outbound_and_list` projected the remote session list and then the
Connected status through two legacy full-record writes. The first write could
become a shell-only update, and a reconnect list rebuild also reset an already
attached foreign pane to `attached=false`.

## Delivered

- Merge list and Connected status/detail into one kernel-authoritative
  HostRecord write before binding the outbound transport.
- Preserve `attached=true` for every existing local foreign pane during a
  reconnect/list refresh.
- Keep test-only local projection explicit through an injectable commit seam;
  production has no kernel-unavailable fallback.
- Add a regression for preserving an existing foreign attachment flag.

## Verification

- `cargo test -p ridge --lib hosts::tests --quiet`: 19 passed.
- No protocol or version change; no publication was made because `v0.1.54`
  consumed today's release allowance.

## Remaining gates

Other status/disconnect/reconnect projections still need full kernel lifecycle
ownership and cross-process evidence; this slice only closes outbound list
ingress.
