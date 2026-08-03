# Iteration 139 - real-process Kernel PTY reattach guard

## Scope

Turn the desktop deep-root PTY contract into a true-process regression. The
desktop proxy may exit; `ridge-kernel` must retain the stable pane-keyed PTY,
accept input while no proxy lease exists, and let a replacement proxy resume
from an explicit output cursor.

## Change

- Added a `ridge-cli` integration test that starts the real detached Kernel,
  creates a stable domain PTY, writes a marker, detaches its output lease,
  writes a second marker while the client-side lease is absent, then attaches
  a replacement lease after the last consumed sequence and verifies the second
  marker is delivered.
- The test destroys only its fixture PTY and lets the existing cleanup stop the
  isolated Kernel, so it does not alter the user's running Ridge instance or
  data.

## Verification

- `cargo test -p ridge-cli --test kernel_lifecycle_e2e --quiet` - 3 passed,
  0 failed (including the new real-process reattach test).
- Existing code path remains unchanged: ordinary desktop shells use the
  Kernel proxy; structured Agent launches remain local because their
  process-specific environment/TMUX contract is different.
- Physical tray exit/restart, WebView2 memory soak, public Remote/Cloud, and
  dual-window/dual-host evidence remain external gates; no release was made.

## Acceptance

The local real-process proof now covers the missing lifecycle fact: dropping a
client lease does not kill the child, and a replacement lease receives output
after a known cursor. It does not claim older-build local ConPTY sessions are
retroactively recoverable.
