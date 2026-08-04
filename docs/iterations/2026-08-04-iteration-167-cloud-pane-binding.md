# Iteration 167 — Stable Cloud Remote pane binding and history reconciliation

## Scope

Close residual item 3 from the previous handoff before starting the formal
release: a detached Cloud Remote session must reattach to the same kernel PTY
without guessing from the first live pane. Reconcile the historical planning
notes at the same time, while retaining append-only evidence and external gates.

## Root cause

`packages/ridge-cli/src/pty.rs` previously selected a live kernel PTY by CWD,
then fell back to the first running PTY. CWD is not a pane identity: two panes
can share it, and a restart can observe unrelated panes first. This could route
Cloud input, resize, or output to another terminal and made multi-pane recovery
non-deterministic.

## Implementation

- Persist a non-secret `remote-cloud-pane.json` binding beside the Cloud auth
  file. The file contains schema, kernel `ptyId`, optional `workspaceId`, and
  CWD only as diagnostic context.
- Let `RIDGE_REMOTE_PTY_BINDING_FILE` override the path; the Tauri Cloud
  supervisor now passes the explicit path to the detached daemon.
- Select in strict order: live persisted PTY with matching workspace (when
  present), live `cloud-remote` launch profile, deterministic same-CWD match;
  otherwise create a new PTY. The old first-live fallback is removed.
- Persist only after the output lease attaches. If persistence fails, detach
  the lease before returning, so a failed reconnect cannot leak a pending
  output subscription.
- Keep the kernel PTY as the owner. Cloud reconnect receives a lease from the
  current output head and never destroys the kernel terminal.

## Verification

| Gate | Result |
| --- | --- |
| `cargo test -p ridge-cli --tests --quiet` | 134 unit + 3 integration passed |
| `cargo test -p ridge-kernel --lib --quiet` | 46 passed |
| `cargo check --manifest-path src-tauri/Cargo.toml --lib --quiet` | passed; existing warnings only |
| `pnpm check` | 0 errors / 0 warnings |
| `git diff --check` | passed |

The new deterministic tests cover persisted identity precedence, rejection of
an unrelated first live PTY, and same-CWD tie-breaking. Packaged Windows
force-kill plus physical phone reconnect remains an external evidence gate;
this archive does not claim that device test.

## Historical requirement reconciliation

- Iterations 165 and 166 remain as immutable evidence and are superseded by
  this snapshot; their completed code is not reopened as a duplicate task.
- `REQ-20260730-01`, `REQ-RDG-REMOTE-CONNECT-01`, and
  `REQ-RIDGE-KERNEL-HOST-01` now record this stable-binding code closure. Their
  physical/public four-path, WebView2 long-run, dual-window, and full-domain
  evidence gates remain explicitly open rather than being misreported as done.
- No historical source note or user data is deleted. Completed work is marked
  by this archive and the current-state index; only genuinely open external
  gates remain actionable.

## Release boundary

Item 3 is code-complete and pushed before release preparation. Formal versioned
release (item 2) starts only from a clean, pushed worktree and must pass the
existing asset matrix; a failed workflow must not be represented as a version
bump or completed release.
