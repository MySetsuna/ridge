# Iteration 90 — Shared sidebar request lifecycle

Date: 2026-08-03  
Baseline: `v0.1.43` (`5c50efd` documentation head; runtime release commit `6ca6f6d`)

## Scope

This slice closes a deterministic stale-response gap left after the Remote
roster/SCM work. Directory navigation, file search, and file/diff viewing can
outlive their component or be superseded by a newer request. A late result
must not overwrite a newer CWD, query, or selected file.

## Landed

- `SidebarFileTree` aborts the previous directory observer, assigns a
  generation, and accepts results only when the observer is current. Destroy
  aborts the observer and invalidates its generation.
- `SidebarSearch` cancels both debounce and in-flight search observers;
  short queries clear state immediately, and stale success/error/finally paths
  cannot update the current search.
- `FileViewer` applies the same generation and abort fence to file reads and
  Git diffs, and clears its copy-reset timer on destroy.
- `SidebarProvider` and the shared sidebar contract now carry optional
  `AbortSignal` values through Remote Query observers and WebSocket data
  provider calls. QueryClient-backed requests remain shared: an individual
  component leaving the view stops observing its result without cancelling a
  request needed by another observer.
- Tauri's API signature accepts the signal for transport parity; the existing
  invoke boundary still cannot interrupt an already-running command.

## Deterministic evidence

- `pnpm exec vitest run src/shared/sidebar/sidebarRequestLifecycle.test.ts src/remote/lib/sidebarProvider.test.ts src/lib/transport/ws.test.ts --reporter=dot` — 3 files, 17 passed.
- `pnpm test -- --reporter=dot` — 140 files, 1468 passed, 1 skipped.
- `pnpm check` — 0 errors, 0 warnings.
- `git diff --check` — clean.

The source-contract test verifies the cancellation, generation guard, and
destroy hooks. It does not claim physical-phone, WebView2 heap, public
WebRTC, or protocol-level host cancellation evidence.

## Residuals carried forward

Production Remote branch identity is still absent from some query callers;
Kernel authority/standalone-host migration, true remote host-task
cancellation, dual-window/Remote workspace singleton E2E, WebView2 long-run
heap evidence, physical mobile `runtime.lastError` attribution, and public
authenticated Git/WebRTC paths remain open. These are not reclassified by
this local lifecycle fence.

## Publication plan

The code commit is `d7c614d`, pushed to `origin/main`. Because code changed
after `v0.1.43`, the next release must be `v0.1.44` and must pass the clean
worktree gate, the version-contract check, the full release matrix with
matching assets, Remote publication, and Cloud health verification.

## Publication closure

Version sources were aligned in `24420a4` and the pre-tag clean-worktree gate
passed with `HEAD == origin/main`. Release workflow `30761202858` completed
successfully across test, Linux, macOS ARM/x64, and Windows. GitHub Release
`v0.1.44` is formal (`draft=false`, `prerelease=false`) with 12 matching
installer/CLI assets: the four `rdg-*` binaries, desktop installers for
Windows/Linux/macOS, and both macOS app archives.

Remote workflow `30762473570` completed successfully and activated the new
desktop/mobile artifact set. Cloud health returned HTTP 200 with
`{"ok":true,"data":{"version":"0.0.7"}}`. The post-release documentation
archive is docs-only; no runtime change was made after the tag.
