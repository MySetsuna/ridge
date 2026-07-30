# Iteration 70 Contract — per-host topology retry and headless audit

- Date: 2026-07-29
- Status: approved / implementation
- Requirements:
  - `REQ-REMOTE-HOST-RETRY-01`
  - `REQ-HEADLESS-DETECTION-02`

## Delivery boundary

1. Per-host topology retry
   - A failed topology refresh retains the last successful workspace/pane tree.
   - Polling stops replaying a host after its topology enters error state.
   - The error row exposes an always-visible host-local action.
   - Retry shares an existing request for that host; other hosts are untouched.
   - Component teardown cancels the caller wait and suppresses stale commit.
   - Authentication/TOTP/401/403 failures direct the user to re-enter connection
     credentials; they are never blindly auto-retried.

2. Headless capability audit
   - `new_headless_session` creates only Ridge-owned sessions in the dedicated
     `headless` socket.
   - `list_native_sessions` projects `native::list_all_sessions`; Hosts and
     Agent Center consume only those DTOs.
   - `summon_native_session` adopts the selected Ridge-owned session into an
     explicit workspace.
   - Arbitrary OS PIDs remain unsupported and are not shown as summonable.

## Non-goals

- No host Ridge launch, shutdown, or process manipulation.
- No credential persistence and no automatic authentication replay.
- No global refresh or second host-tree cache.
- No rename/removal of the headless entry; that requires separate approval.

## Deterministic gates

- Host forest isolation, stale-tree retention, cancellation, and error
  classification tests.
- Full Svelte/TypeScript diagnostics.
- Existing Rust focused session aggregation test and compile.
- `git diff --check`.

## User-track gate

- A real Ridge-owned tmux/headless session must be created, listed, summoned,
  and detached on the user's installed host. This turn intentionally does not
  start or stop that host.

