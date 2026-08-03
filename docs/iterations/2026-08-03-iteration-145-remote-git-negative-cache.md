# Iteration 145 — Remote Git negative-cache convergence

Date: 2026-08-03

## Scope

Stop repeated Remote/mobile Git probes after a host has confirmed that a
directory is not a Git repository. The result must survive sidebar/provider
remounts for the current transport session, while a different directory keeps
its own detection lifecycle.

## Implementation

- `src/remote/lib/sidebarProvider.ts` now keeps a bounded session-scoped
  negative-root map keyed by `(remote session, normalized root)`.
- Explicit `not a git repository` failures and successful
  `is_git_repo: false` responses both populate the map.
- The legacy desktop adapter, which has no stable session identity, remains
  provider-local to avoid cross-host false positives.
- A reset helper is exported for deterministic tests/HMR; the production map
  is bounded to 128 roots.

## Acceptance

- Recreating a Remote sidebar provider for the same session/root returns the
  cached empty Git snapshot without another RPC.
- Changing the root performs a new detection.
- Transport/time-out errors are not converted into a negative repository
  result.

## Evidence

- `pnpm exec vitest run src/remote/lib/sidebarProvider.test.ts --reporter=dot`
  — 15 passed.
- Full `pnpm check` and repository test matrix remain required before commit.

## Boundaries

This closes only the Remote sidebar negative-cache gap. Public/physical-phone,
WebView2 heap soak, and full Kernel authority evidence remain open; no release
or version bump is made.
