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

## Iteration addendum: Mobile Remote backpressure

The user pre-approved `REQ-MOBILE-REMOTE-BACKPRESSURE-01` for this same
iteration after reporting intermittent mobile Remote freezes. The active
requirement covers a bounded per-pane render queue, frame-time budgeting,
input/control priority, overflow telemetry, and complete queue/timer cleanup on
clear, reconnect, or pane destruction. The existing `PaneRpcScheduler` and
`paneInputGate` remain the input/RPC authority; this addendum closes the render
feed path that still performed an unbounded synchronous deferred drain.

The desktop tag attempt `v0.1.57` was rolled back after the release test gate
failed because the Linux Tauri check lacked the generated resource
`binaries/rdg-x86_64-unknown-linux-gnu`. Version remains `0.1.56`; no failed tag
or empty release is claimed. The Remote/cloud workflow did activate
`0.1.57+g7ce4386` successfully, so its exact artifact/version asymmetry is
recorded as an external release-state fact rather than a desktop-release claim.

## Iteration addendum: Mobile Remote live tail and pane-grid invariant

The user also approved two Remote correctness requirements for this iteration:

- `REQ-MOBILE-REMOTE-LIVE-TAIL-01`: the live PTY listener is attached before the
  bounded visual seed. Bytes arriving during the seed render immediately and a
  bounded FIFO replay copy is retained for a late seed reset; if that copy fills,
  the stale seed is skipped. Older history is a separate, incremental scroll-up
  query and cannot block the live tail or input lane.
- `REQ-REMOTE-PANE-GRID-INVARIANT-01`: the current pane content box is the only
  local geometry authority. A host `pty-resized` event now triggers a local fit
  instead of injecting its possibly stale rows/cols, and the refresh action
  bypasses debounce through `fitPaneNow` so a smaller shell grid can be repaired.

Deterministic coverage includes live-before-seed visibility, bounded seed/replay
size, seed-reset ordering, pane geometry capacity, and stale-grid local
authority. Physical mobile/PWA and cross-host soak evidence remains an external
gate; no release closure is claimed from unit tests alone.

## Iteration addendum: release gate remediation

The prior `v0.1.57` desktop attempt failed before the matrix build because the
Linux Tauri compile check referenced `binaries/rdg-x86_64-unknown-linux-gnu`
without generating it. A second matrix attempt exposed the same missing target
sidecar for Intel macOS. `.github/workflows/release.yml` now builds the Linux
sidecar in the test gate and explicitly stages
`rdg-x86_64-apple-darwin` before the Intel Tauri build. This closes the known
deterministic CI blockers; the versioned release still requires a clean pushed
tree, successful test gate, all matrix assets, and the separate Remote/cloud
artifact audit.
