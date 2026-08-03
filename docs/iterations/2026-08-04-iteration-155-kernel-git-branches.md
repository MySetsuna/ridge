# Iteration 155 - Kernel-owned Git branch reads

Date: 2026-08-04
Status: focused code green; full regression and physical gates remain open.

## Scope

Close the remaining high-frequency desktop SCM read path for branch lists.
`SourceControl` and `PaneGitPill` already cache branch data for five minutes,
but their Tauri command still spawned Git locally outside the kernel domain.

## Change

- Added authenticated `GET /v1/domain/git/branches?path=...`.
- Kernel performs repository detection before `git branch`, so confirmed
  non-Git roots return a healthy negative result without repeated Git errors.
- Added typed `read_domain_git_branches` decoding with source validation,
  malformed-response rejection, and Windows-safe query encoding.
- Desktop `git_list_branches` now uses the kernel adapter while preserving the
  existing Tauri signature and UI cache behavior.
- Shared `BranchInfo` derives `Deserialize` for the typed kernel response.

## Verification

- Kernel client branch decoder contract: 1 passed.
- Kernel non-Git branch domain test: 1 passed.
- `cargo check -p ridge --lib`: exit 0 (pre-existing warnings only).

## Remaining gates

Run the full Rust/TypeScript matrix and exercise branch reads against real Git
and non-Git roots. Git mutation authority, filesystem/Agent/Remote domains,
PTY orphan/health evidence, physical/public Remote, WebView2 heap soak, and
dual-window/Host remain open. No version bump or release was made.
