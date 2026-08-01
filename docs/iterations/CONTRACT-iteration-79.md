# Iteration 79 Contract — converged Kernel, SCM, PTY, and release closure

- Date: 2026-08-01
- Status: completed for the approved code and deployment scope; physical-device and long-run telemetry remain explicit carry-over evidence.
- Baseline: `7d27ab1`
- Final product tag: `v0.1.23` at `d1f6cd9`
- Remote artifact: `0.1.23+gd1f6cd9`
- Ridge-cloud deploy workflow: `30700070947` (success)
- Release workflow: `30701280050` (success; all four matrix jobs)
- Remote publish workflow: `30701281101` (success; desktop/mobile indexes verified)

## Reconciled scope

| Area | Result | Evidence |
| --- | --- | --- |
| Shared Kernel/MCP | Removed the duplicate kernel MCP transport; mounted the shared `ridge-mcp` HTTP/WS router and host-owned session state. | `7d27ab1`; kernel, MCP, bridge tests; real split/capture/delegate/inbox E2E |
| PTY rendering and clear | One bounded `ridge_term::Terminal` renderer per PTY, capped raw scrollback, resize propagation, and true clear of screen plus retained output. | Kernel unit tests and lifecycle E2E |
| Pane lifecycle | Split/close/destroy paths mark closing before kill, cancel scoped pending work, retire desktop write lanes, and reject late Pane-not-found noise case-insensitively. | `7d27ab1`, `ed094ba`, `4739d56`; ptyBridge and kernel lifecycle tests |
| RPC admission | Input is ordered and bounded; resize is latest-value/debounced; timeout failures back off and pause after a threshold; scope retirement cancels pending requests. | scheduler/client tests; 1,000-burst and timeout regression fixtures |
| SCM | Shared single-flight and normalized non-Git negative cache; sync and async Git paths use the same semaphore, timeout, process-tree kill, and diagnostics; watcher debounce state is reclaimed. | `dc1471a`, `4739d56`, `cca6483`; 308 ridge-core tests and 20 SCM tests |
| Error aggregation | Repeated identical errors preserve the first event and emit a timed count summary; Pane-not-found is non-actionable noise. | `2052753`, `4739d56`; repeated-error and ptyBridge tests |
| Terminal memory | Scrollback, renderer, worker, and hidden-pane lifetimes are bounded and reclaimed; desktop write queue has a hard cap. | `7043ff5`, `ed094ba`; memory and queue tests |
| Host, workspace, Commune, Git pill | Non-blocking host progress, measured attach resize, cross-window workspace singleton, persistent Commune entry, and one Git pill render site remain covered. | `7b7daee`, `c290143`, `d2dec7e`, `8b2c6a4`; focused UI/ownership tests |
| Mobile `runtime.lastError` | No project build input uses Chrome Extension Messaging. PWA service-worker messaging is standard `Client.postMessage`; no business-code suppression or fake response wrapper was added. | `CONTRACT-iteration-78.md`; source and bundle audit |

## Verification

- `pnpm test`: 116 files, 1,350 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `cargo test -p ridge-core --lib`: 308 passed.
- `cargo test -p ridge-kernel --lib`: 14 passed.
- `cargo test -p ridge-cli --test kernel_lifecycle_e2e`: 1 passed.
- Release matrix: Linux, macOS arm64/x64, and Windows all passed.
- Release assets: 11 version-matched installers/archives/CLI binaries; Release is published, not draft.
- Remote artifact upload activated `0.1.23+gd1f6cd9`; public health endpoint returned HTTP 200.
- Deterministic LAN Remote E2E rerun at 2026-08-01 22:07 HKT: desktop and mobile both passed (`canvas=true`, mobile `tree=true`, WS connected, no browser errors); evidence: `.iteration/artifacts/rdg-remote-e2e/last-result.json`.

## Explicit carry-over evidence

These require real external evidence and are not falsified by fixtures:

- affected physical phone: first warning source URL plus clean-profile/incognito and one-by-one injector A/B;
- long-run WebView2 private-bytes/heap curve during output, hide/show, clear, and pane destruction;
- public WebRTC/TURN latency, CPU, bandwidth, and reconnect measurements;
- physical two-window focus/close race and dual-Host drag/resize run.

The isolated `scripts/remote-gc-e2e.mjs` attempt was not counted as product evidence: the temporary `rdg` LAN host intentionally rejects `list_saved_workspace_files` and exposes no create-pane/create-workspace controls, while the fixture blocks service workers. This is a harness/host mismatch, not a pass or a suppressed warning.

No carry-over is marked complete without its corresponding runtime trace or measurement.
