# Iteration 160 — Kernel Git read/write authority

## Scope

`PaneGitStatus` already routed status and branch reads through the long-lived
kernel, but its second high-frequency read (`git_diff_summary`) still invoked
the local Tauri/core Git path. The desktop write commands also bypassed the
kernel, leaving two process schedulers for the same repository and allowing
Git contention to return.

## Change

- Added authenticated `GET /v1/domain/git/diff-summary?path=...`.
- Kernel detects the repository before calling `git_diff_summary_sync`; a
  confirmed non-Git path returns `{is_repo:false, summary:{added:0,removed:0}}`.
- Added typed, source-checked `read_domain_git_diff_summary` decoding.
- Desktop `git_diff_summary` is now a bounded `spawn_blocking` kernel adapter;
  the existing Tauri signature and frontend Query/slot contract remain stable.
- `GitDiffSummary` now derives `Deserialize`/`Eq` for the shared contract.
- Added an authenticated tagged `POST /v1/domain/git/mutate` route for stage,
  unstage, commit, checkout, push, and push-branch. Non-Git roots fail before
  any write; arbitrary Git argv cannot cross the domain boundary.
- Desktop wrappers for those six operations now call the kernel route; direct
  core calls remain only for Git operations not yet included in this slice.

## Verification

- Kernel diff-summary and mutation decoder contracts: 2 passed.
- Kernel non-Git diff-summary and mutation guard domain contracts: 2 passed.
- Full Rust: ridge 256, ridge-kernel 41, ridge-core 315 passed.
- Full Vitest: 148 files, 1541 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `git diff --check`: passed (LF/CRLF conversion warnings are checkout
  normalization only).

## Remaining external gates

Remaining direct Git writes and graph/history reads still need kernel authority;
physical/public Remote, WebView2 heap soak, dual-window/Host, and complete
domain-authority evidence remain open. No version bump or release was made.
