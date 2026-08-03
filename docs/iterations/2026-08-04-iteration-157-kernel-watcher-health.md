# Iteration 157 - Kernel watcher health and shutdown ordering

Date: 2026-08-04
Status: focused code green; physical process and tray evidence remain open.

## Scope

Close two lifecycle races found in the desktop shell. The death watcher only
checked a PID, so PID reuse or a permanently unhealthy endpoint could leave a
stale shell alive. The tray kernel-quit path set `quitting` after shutdown,
allowing the watcher to race a second exit.

## Change

- The watcher captures the authenticated kernel endpoint, probes process and
  health together, tolerates transient health failures, and exits only after
  three consecutive health failures (or immediate process death).
- Health failure state resets on recovery; the threshold is a pure helper with
  deterministic tests.
- `MENU_ID_QUIT_KERNEL` now atomically marks `quitting` before shutdown,
  suppressing watcher callbacks during the intentional stop. A failed shutdown
  rolls the state back so the user can retry.
- Ordinary desktop exit still leaves the singleton kernel alive for restart
  reattach.

## Verification

- `cargo test -p ridge --lib kernel_lifecycle::tests`: 7 passed.
- Tests cover process death, transient failures, threshold exit, and health
  recovery reset.

## Remaining gates

Run the full Rust/TypeScript matrix and physical tray quit/desktop restart
checks. Validate kernel health under a real transient network fault and confirm
the same PTY PID/UUID reconnects. Orphan recovery UX, public/physical Remote,
WebView2 heap soak, dual-window/Host, and complete domain authority remain open.
No version bump or release was made.
