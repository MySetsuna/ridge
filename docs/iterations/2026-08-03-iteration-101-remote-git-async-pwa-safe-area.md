# Iteration 101 — Remote Git async first paint and PWA connection banner safe area

Date: 2026-08-03  
Status: implementation complete; release deferred by the daily publication freeze

## Root causes

- Remote `git_status` ran SCM status, 50-commit history, and branch enumeration
  serially before returning one response. A mobile Git tab therefore waited on
  the heaviest Git work even though the working-tree view did not need history.
- Reconnect and reconnect-failure banners were direct children of the fixed
  PWA root without a top safe-area inset. On notch devices the banner text and
  retry action were covered by the cutout.

## Delivered

- Remote first Git query now requests working-tree state only. Graph history
  and branches use separate cancellable `gitGraph` data requests, with stable
  Query keys, single-flight caching, scoped invalidation, and legacy-host
  fallback.
- Graph requests share the transport AbortSignal; cancellation sends a
  `data-cancel` for both branch and history RPCs. The host dispatches both
  read methods through the existing Git request guard.
- The mobile Git panel yields one animation frame before its first RPC, keeps
  the drawer interactive while loading, and loads Graph data only after the
  user selects Graph. Stale responses are fenced by generation and teardown.
- Reconnect and failure banners reserve `env(safe-area-inset-top)` and a
  minimum inset-aware height in browser and standalone PWA modes.

## Verification

- Full Vitest: 144 files, 1489 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- Targeted transport/sidebar/PWA tests: 26 passed.
- `cargo test -p ridge --lib remote_host_impl --quiet`: 12 tests passed.
- `pnpm build:remote:mobile`: production PWA build succeeded; service worker
  generated with 36 precache entries.
- Commits pushed to `origin/main`: `16a34bf`, `d8ae245`.

## Release and remaining gates

The one authorized publication window for today was already used by the
`v0.1.54` desktop matrix and Remote/Cloud workflow before this follow-up. No
second release or artifact overwrite was performed. These commits are queued
for the next allowed release window. Physical notch-device/PWA interaction,
public Cloud/WebRTC evidence, WebView2 heap soak, branch-authoritative Query
key population, and dual-window/dual-Host proof remain external gates.
