# Iteration 144 — Kernel-owned structured Agent launch

Date: 2026-08-03 (Asia/Hong_Kong)

## Scope

Close the remaining PTY authority split identified after ordinary desktop and
`rdg` shells moved to `ridge-kernel`. Structured Agent launches previously
constructed a native PTY in Tauri, so their child process, environment, output
replay, and destroy path differed from ordinary panes.

## Implemented

- Added an authenticated Kernel domain launch contract with explicit `program`,
  `args`, and bounded `env` fields. `shell` remains backward compatible.
- Added Kernel-side `PtyBridge`/`PtyRegistry` argv+environment propagation.
- Added stable-pane client helper `create_domain_pty_with_command`.
- Structured Agent panes now try Kernel first, injecting the existing teammate
  URL/token, TMUX metadata, workspace/pane identity, sidecar, and tmux shim
  PATH. Kernel failure retains the existing local pending-spawn fallback.
- Kernel success resolves the existing `ready_tx` immediately, so teammate
  split callers do not wait for the native 3-second activation path.
- Existing `activate_pane_pty` dimensions remain authoritative: a Kernel pane
  is attached immediately and later resized to the real frontend dimensions.
- Added argument/environment count and byte caps before any child process is
  spawned.

## Verification

- `cargo test -p ridge-kernel --lib --quiet`: 33 passed.
- `cargo test -p ridge --lib --quiet`: 252 passed.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm test -- --runInBand`: 147 files, 1529 passed, 1 skipped; exit 0.
- `git diff --check`: passed before commit.

## Boundary / remaining gates

This closes the code path for structured Agent launches but does not claim
physical tray-exit/restart, WebView2 long-run heap, public Remote, or dual
window/dual-host evidence. No version bump or publication is made: today’s
release allowance was already consumed by `v0.1.54`.
