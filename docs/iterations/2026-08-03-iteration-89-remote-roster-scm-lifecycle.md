# Iteration 89 — Remote roster and SCM request lifecycle

Date: 2026-08-03  
Baseline: `v0.1.42` (`6dc8572` documentation head; runtime code `bd60f82`)

## Landed

- Remote Agent roster refresh now has a scope fence keyed by remote session,
  workspace, pane identity, and pane CWD. Workspace/pane/CWD changes, reconnect,
  and capability renegotiation start a fresh refresh; stale generations are
  aborted and cannot update the new drawer. Teardown cancels the active scope,
  timers, listeners, and transient Pane attention.
- Remote roster/history Query wrappers observe AbortSignal and reject late
  snapshots even where the legacy `RemoteLink` method has no signal parameter.
  The transport promise remains bounded by its existing timeout; this is an
  observer/generation fence, not a claim that every legacy host task is killed.
- Remote SCM now caches a confirmed non-Git CWD for the lifetime of its provider
  root. Only an explicit `not a git repository` result enters that negative
  cache; timeout, disconnect, and cancellation errors remain visible and
  retryable. This prevents repeated invalid SCM probes in non-Git directories.
- Remote Git stage/commit/push cancellation signals now reach the WebSocket data
  provider, which removes local pending entries immediately. The Git panel keeps
  its busy guard until the underlying call settles, preventing cancel/retry
  overlap. Tauri IPC accepts the signal for API parity but cannot interrupt an
  already-started invoke.
- Desktop shared-workspace panels now sit under a layout-level QueryClient,
  eliminating `useQueryClient` context failures when the cloud resource panel
  mounts.

## Deterministic evidence

- `pnpm test -- --reporter=dot`: 139 files, 1465 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm e2e:rdg-lan -- --skip-build`: desktop and mobile LAN paths passed;
  `canvas=true`, `ws=true`, `browserErrors=[]`, input and resize true. The
  latest machine-readable result is
  `.iteration/artifacts/rdg-remote-e2e/last-result.json`.
- `pnpm verify:pwa`: standalone manifest, scope, icons, service worker,
  safe-area CSS, and no in-app install hook all passed.
- Focused Remote roster/Query/SCM/transport tests pass (47 tests in the
  pre-release audit set); `git diff --check` is clean.

## Release gate and residuals

Runtime commit `d14c812` is pushed to `origin/main`. Before versioning a new
release, the worktree must contain no useful uncommitted or untracked changes;
the release gate remains `git diff --exit-code`, `git diff --cached --exit-code`,
`git ls-files --others --exclude-standard`, and `HEAD == origin/main`.

Evidence still intentionally external or partial: physical-phone Chrome
`runtime.lastError` attribution, public WebRTC/authenticated Remote Git UI,
WebView2 long-run heap/RSS soak, dual-window/Host singleton device E2E, full
Kernel authority migration, production branch identity injection into Remote
Query keys, and protocol-level cancellation of already-running host data tasks.
These are not hidden by local green tests or LAN success.

## Publication closure

The first `v0.1.43` attempt (`30759401836`) failed its version contract because
the root `Cargo.lock` still declared `ridge` as `0.1.42`. The failed tag was
deleted, the version bump was reverted, and a corrected bump aligned
`package.json`, Tauri, Cargo, and the root lockfile before retagging.

The corrected commit is `6ca6f6d`. Release workflow `30759507144` passed the
test gate and Linux, macOS ARM/x64, and Windows builds. GitHub Release
`v0.1.43` is formal (`draft=false`, `prerelease=false`) with 12 matching
installer/CLI assets. Remote workflow `30759691020` activated
`0.1.43+g6ca6f6d`; its desktop/mobile index checks passed. Cloud health returned
HTTP 200 (`version=0.0.7`).

The final release gate is clean: tracked/staged diffs and non-ignored
untracked-file checks are empty, and `HEAD == origin/main`. The remaining
external gates above are carried forward rather than reclassified as complete.
