# Iteration 160 — Kernel Git diff-summary authority

## Scope

`PaneGitStatus` already routed status and branch reads through the long-lived
kernel, but its second high-frequency read (`git_diff_summary`) still invoked
the local Tauri/core Git path. That split authority could recreate Git process
contention and made non-Git noise harder to suppress.

## Change

- Added authenticated `GET /v1/domain/git/diff-summary?path=...`.
- Kernel detects the repository before calling `git_diff_summary_sync`; a
  confirmed non-Git path returns `{is_repo:false, summary:{added:0,removed:0}}`.
- Added typed, source-checked `read_domain_git_diff_summary` decoding.
- Desktop `git_diff_summary` is now a bounded `spawn_blocking` kernel adapter;
  the existing Tauri signature and frontend Query/slot contract remain stable.
- `GitDiffSummary` now derives `Deserialize`/`Eq` for the shared contract.

## Verification

- Kernel diff-summary decoder contract: 1 passed.
- Kernel non-Git diff-summary domain contract: 1 passed.
- Full Rust: ridge 256, ridge-kernel 39, ridge-core 315 passed.
- Full Vitest: 148 files, 1541 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `git diff --check`: passed (LF/CRLF conversion warnings are checkout
  normalization only).

## Remaining external gates

Git mutations/graph still need kernel authority; physical/public Remote,
WebView2 heap soak, dual-window/Host, and full domain-authority evidence remain
open. No version bump or release was made.
