# Iteration 107 - Kernel host session transaction

Date: 2026-08-03
Status: implementation complete; release deferred by the daily publication freeze

## Root cause

`session.attached` was previously persisted by posting a complete `HostRecord`.
Two windows could read the same record, change different session flags, and
overwrite one another. The shell transaction was serialized only in-process;
the kernel had no atomic session transition or duplicate-attach rejection.

## Delivered

- Added authenticated kernel `remote-host-sessions/attach` and `detach`
  endpoints. Each transition clones the topology, validates host/session and
  current state, persists, then swaps under the kernel topology lock.
- Duplicate attach, duplicate detach, unknown host/session, and empty IDs fail
  without changing the persisted or in-memory topology.
- Added typed `ridge-kernel` client helpers and changed the desktop checked
  session flag path to project locally only after the kernel returns a matching
  mutation.
- The existing local attach transaction and duplicate-pane guard remain in
  force, so PTY/sink/layout rollback still covers later failures.

## Verification

- `cargo test -p ridge-kernel --lib --quiet`: 24 passed.
- `cargo test -p ridge --lib hosts::tests --quiet`: 16 passed.
- Kernel handler regression covers attach, duplicate rejection, detach, and
  persisted topology state.
- No version bump or publication was made because `v0.1.54` consumed today's
  release allowance.

## Remaining gates

Live PTY ownership, transport lifecycle, and cross-process workspace/window
claims still need migration behind `ridge-kernel`; no public release claim is
made for those external/no-Tauri/device gates.
