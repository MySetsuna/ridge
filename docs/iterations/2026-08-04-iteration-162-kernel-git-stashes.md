# Iteration 162 — Kernel Git stash-list authority

## Scope

The Source Control panel already skipped stash loading for cached non-Git
roots, but its `git_stash_list` command still went straight from Tauri into
the core Git scheduler. That left a second process boundary and made a stale
or misclassified root capable of emitting another Git error.

## Change

- Added authenticated `GET /v1/domain/git/stashes?path=…`.
- Kernel performs repository detection before `git stash list`; confirmed
  non-Git roots return `{is_repo:false, stashes:[]}` without spawning Git.
- Added a typed, source-checked `StashEntry` response contract and kernel
  client adapter.
- Desktop `git_stash_list` now calls the kernel adapter while preserving the
  existing Tauri signature; no hidden local fallback remains.

## Verification

- Kernel non-Git stash guard: 1 passed.
- Kernel stash decoder contract: 1 passed.
- `cargo check -p ridge --lib`: passed (pre-existing warnings only).
- Full Rust: ridge 256, ridge-kernel 44, ridge-core 315 passed.
- Full Vitest: 148 files, 1541 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- Requirements and iteration gates passed; `git diff --check` passed.

## Remaining external gates

Remaining direct Git writes and graph/history reads still need kernel authority;
physical/public Remote, WebView2 heap soak, dual-window/Host, and complete
domain-authority evidence remain open. No version bump or release is made.
