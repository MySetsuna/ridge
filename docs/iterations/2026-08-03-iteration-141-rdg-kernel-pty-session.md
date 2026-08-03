# Iteration 141 — rdg Kernel-owned TUI PTY session

Date: 2026-08-03
Status: code complete; physical restart and public Remote evidence pending.

## Problem

`rdg` TUI and LAN-host workspace sessions still used the in-process
`ridge_kernel::pty::PtyRegistry`, while the desktop shell path already used the
long-lived `ridge-kernel` process. That split allowed the headless shell to own
children that could not survive the shell process and left two PTY lifecycle
implementations to maintain.

## Change

- `packages/ridge-cli/src/tui/session.rs`
  - production `LocalPtySession` now creates or attaches a stable Kernel domain
    PTY, writes/resizes through the authenticated Kernel client, and polls a
    bounded output lease on a cancellable thread;
  - dropping the shell proxy stops polling and detaches the lease without
    destroying the Kernel-owned child;
  - a failed first attach destroys only the newly-created PTY, preventing an
    orphan; existing PTYs remain untouched;
  - the in-process bridge remains test-only so unit tests never boot a detached
    Kernel process.
- `packages/ridge-cli/src/tui/workspace.rs`
  - PaneTree assigns the stable UUID before PTY creation;
  - layout changes are prepared on a clone and committed only after Kernel PTY
    creation succeeds, preventing a pane-without-session race;
  - pager and LAN-host workspace sessions use the same Kernel-backed session.

## Verification

- `cargo check -p ridge-cli --bin rdg`: passed (pre-existing warnings only).
- `cargo test -p ridge-cli --bin rdg --quiet`: 127 passed, 0 failed.
- `cargo test -p ridge-cli --test kernel_lifecycle_e2e --quiet`: 3 passed,
  including detached output lease and stable PTY replay.
- `pnpm check`: 0 errors, 0 warnings.
- `git diff --check`: passed.

## Not claimed

The desktop restore command already invokes `reattach_kernel_ptys` after saved
workspaces are restored; physical tray-exit/restart, WebView2 heap soak, and
public Remote/dual-host runs remain environment gates. No version bump or
publication was made because `v0.1.54` consumed today's allowance.
