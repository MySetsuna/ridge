# Iteration 106 - Host attach transaction hardening

Date: 2026-08-03
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`attach_host_session` mutated the workspace layout and host attachment state
before all fallible setup had completed. A failed outbound `subscribe` could
leave a new split behind, while `openpty`/`take_writer` failures were silently
ignored and still returned a pane id. The result was a foreign session with no
renderable local terminal, stale stdin sinks, or a subscription that had no
local owner.

## Delivered

- Validate the target workspace and first leaf; missing/empty workspaces now
  fail closed instead of receiving a random UUID.
- Construct the local foreign PTY before mutating layout or host state; PTY and
  writer errors are returned to the caller.
- Subscribe before splitting. Split/terminal-install failures undo the remote
  subscription and remove every local side effect (layout leaf, terminal,
  sink, and foreign metadata).
- Serialize attach transactions and reject an already-attached remote session,
  so concurrent windows cannot overwrite the first pane's live input sink.
- Commit `session.attached` through the kernel-authoritative host mutation path;
  an unavailable/rejecting kernel aborts the attach without publishing a
  shell-only flag.
- Added a deterministic regression proving rollback clears a split, PTY,
  foreign attachment, sink, and outbound subscription together.

## Verification

- `cargo test -p ridge --lib hosts::tests::rollback_host_attach_clears_every_partial_side_effect --quiet`: 1 passed.
- `cargo test -p ridge --lib hosts::tests --quiet`: 16 passed.
- `cargo check -p ridge --lib`: passed; existing Rust linker/dead-code
  warnings remain.
- No version bump or publication was made because `v0.1.54` consumed today's
  release allowance.

## Remaining gates

The attach transaction now fails closed locally and at the kernel topology
commit boundary. Full kernel ownership still requires moving live PTY/session
status and transport lifecycle into the kernel, then proving no-Tauri
desktop-exit and rdg attach/reconnect behavior.
