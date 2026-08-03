# Iteration 161 — Kernel Git fast-status authority

## Scope

Remote first paint still called `get_scm_status_fast` directly in the Tauri
shell. That bypassed the authenticated kernel Git domain even though the full
desktop status path had already converged, leaving a second scheduler for the
same repository.

## Change

- Added an optional `fast` flag to the kernel Git status query. The default
  remains the full status contract; `fast=1` calls
  `get_scm_status_fast_sync` and skips both `git diff --numstat` children.
- Added a typed kernel client adapter for the fast projection and a URL
  contract test that preserves path encoding and the fast marker.
- Remote/Tauri `get_scm_status_fast` now uses the authenticated kernel route;
  no local fallback or duplicate Git scheduler remains in this adapter.
- Non-Git detection happens before status work, so a confirmed non-Git root
  returns a typed negative response without starting Git.

## Verification

- Kernel fast-status non-Git guard: 1 passed.
- Kernel fast-status URL contract: 1 passed.
- `cargo check -p ridge --lib`: passed (pre-existing warnings only).
- Full Rust: ridge 256, ridge-kernel 42, ridge-core 315 passed.
- Full Vitest: 148 files, 1541 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- Requirements and iteration gates passed; `git diff --check` passed.

## Remaining external gates

Remaining direct Git writes and graph/history reads still need kernel authority;
physical/public Remote, WebView2 heap soak, dual-window/Host, and complete
domain-authority evidence remain open. No version bump or release is made.
