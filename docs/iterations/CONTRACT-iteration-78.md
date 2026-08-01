# Iteration 78 Contract — Remote stability, resource bounds, and release closure

- Date: 2026-08-01
- Status: completed; physical and production telemetry carry-over remains explicit below
- Baseline: `44609d9`
- Product tag: `v0.1.19` at `465e1b4`
- Requirements: `REQ-20260730-01`, `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`, plus approved carry-over selected by `INTAKE-20260801-GOAL-CONTINUE-01`
- Authority: current code, deterministic tests, captured browser runs, and published artifacts; NotebookLM remains planning context only

## Three-source reconciliation

| Concern | Prior state | Iteration 78 result | Evidence |
| --- | --- | --- | --- |
| Remote RPC and pane lifecycle | Partial guards existed; mobile and desktop paths could still diverge | One pane-scoped scheduler contract now bounds input/resize, merges duplicate work, backs off timeouts, and cancels on destroy | `1495c5a`, `8f97c64`, `8c61182`; focused scheduler/lifecycle tests |
| SCM polling and logs | Multiple callers duplicated status/branch/stash work; negative detection ownership was incomplete | Cross-component single-flight, normalized repository identity, retained non-Git admission, and aggregated repeated failures | `cb24cb3`; 42 focused tests |
| Git child processes | Three production paths bypassed the shared cap or tree-kill path | Every production Git spawn now uses the process-wide admission, wall timeout, cancellation/tree kill, and diagnostics exit; static bypass guard added | `3c888f7`; 307 ridge-core tests, including real hanging child and active-count recovery |
| Terminal memory and clear | Per-pane bounds existed, but aggregate pressure and hidden renderers remained resident | Device-aware aggregate scrollback budget, pressure trimming, hidden renderer parking, lifecycle-safe restore; true-clear chain retained | `7043ff5`; 73 focused tests |
| Remote Host | Slow nodes blocked topology publication; attach resize could race DOM geometry | Incremental topology, per-host progress/error state, non-blocking connect, unified measured size claim with bounded retry | `9240c8c`; 43 focused tests |
| Multi-window workspace singleton | Frontend scanned and claimed workspaces one by one, allowing focus-steal races | Atomic backend acquisition selects an existing/free workspace without stealing another window's owner | `d2dec7e`; 61 frontend and 4 Rust ownership tests |
| Agent's Commune | Feature existed but lacked visible browser proof | Persistent disabled-state entry and members/groups/history reachability are browser-gated | `8b2c6a4`; Playwright 3/3 |
| PaneHeader Git-pill | Reported as possibly layered | Audited as one render site and exactly one pill per PaneHeader; multiple repositories use a selector, not stacked pills | `SplitContainer.svelte`; 13/13 pane Git tests; zero product diff |
| Phone `runtime.lastError` | Repeated warning attribution unknown | Repository contains no Chrome Extension Messaging API; clean isolated mobile Remote run emitted zero browser errors. Project ownership is excluded unless an affected-device source URL proves otherwise | static source audit plus real `rdg` mobile browser run |
| Release reliability | Release metadata could drift and platform-only code was not exercised early | Unix liveness and cross-platform termination were centralized; package/Tauri/Cargo/lock versions are checked before Release and Remote publish; the release gate now compiles the Tauri library natively on Linux before starting the matrix | `a87cec9`, `a363788`, `68d650d`; local WASM build, CI version gate, and four-platform Release success |

## Deterministic acceptance

- Vitest: 116 files, 1348 passed, 1 skipped.
- Svelte: 0 errors, 0 warnings.
- `ridge-core`: 307 passed; timeout kill-tree, shared concurrency peak, real Git, and active-child recovery included.
- Tauri library: 223 passed; rollback and workspace ownership included.
- `ridge-kernel`: 12 passed on Windows; Linux and macOS builds passed in the Release matrix.
- Commune Playwright: 3/3 passed.
- Remote desktop/mobile production builds: exit 0.
- Requirements gate, strict preflight, iteration gate, release-version contract, Cargo metadata, and diff check: exit 0.

## Runtime and deployment evidence

- Real headless `rdg.exe` LAN host: protocol verification passed; isolated desktop and mobile Chromium both established authenticated WebSockets and rendered a terminal canvas; mobile workspace tree rendered; browser error arrays were empty. Evidence timestamp: 2026-08-01T08:07:48Z.
- The LAN harness now allocates only browser-safe ports; the original `ERR_UNSAFE_PORT` reproducer is closed by `75bf84f`.
- Remote Cloud workflow `30693132394` for `465e1b4` built both bundles, atomically published them, verified desktop/mobile indexes, and probed the public favicon.
- Release workflow `30693127672` passed its test gate and all four build jobs. `v0.1.19` is published with version-matched Windows `.exe`/`.msi`/`rdg`, Linux `.deb`/`.AppImage`/`rdg`, and macOS Apple Silicon/Intel `.dmg`/app archives plus macOS-arm64 `rdg`.

## Carry-over requiring physical or production telemetry

These are not replaced by fixtures and remain explicit next-iteration inputs:

- affected physical phone extension/incognito A/B if the Chrome extension warning reappears;
- long-run WebView2 private-bytes/heap curve under real terminal output, hide/show, clear, and pane destruction;
- public WebRTC/TURN latency, CPU, network, and reconnect A/B on a real remote network;
- physical two-window focus/close race and dual-Host drag/resize run.

No code item above may be reopened from documentation alone: a regression requires a reproducer, failing gate, or captured runtime trace.
