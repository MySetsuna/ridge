# Iteration 80 Contract — bounded pane RPC, SCM quiescence, and release convergence

- Date: 2026-08-01
- Scope: approved stability/performance work carried from iteration 79.
- Code closure: `8b00138` plus `b03a7f1`, `76eb2ed`, `2eee0e9`.
- Release line: `v0.1.24` tag pushed; GitHub Release, Remote artifact, and ridge-cloud deploy remain workflow-gated until assets and health checks are verified.
- Next-iteration intake: `PENDING-REQ-20260801-AGENT-COMMUNE-UI-01` (recorded locally; not implemented or synced).

## Reconciled matrix

| Area | Current result | Evidence | Remaining proof |
| --- | --- | --- | --- |
| Pane input/resize RPC | Per-pane ordered input, latest-value resize, bounded queues, 5 s timeout, exponential backoff, pause threshold, scope cancellation. | `packages/remote/src/shared/transport/paneRpcScheduler.ts`; 21 scheduler/remote tests; full Vitest. | Public-network latency/CPU and physical mobile trace. |
| Stale Pane lifecycle | Destroy/prune/disconnect retires lanes; remote resize now returns stale-Pane errors instead of false success. | `src-tauri/src/commands/terminal.rs`, `remote_bridge.rs`, `remote_host_impl.rs`; Rust JSON-RPC 8/8. | Long-running pane destroy/recreate trace. |
| SCM polling/logs | Non-Git negative cache and branch TTL; repeated errors aggregate by level and count. | `SourceControl.svelte`, `repeatedError.ts`; targeted SCM/log tests. | Five-minute public-session telemetry. |
| Terminal memory/clear | Bounded scrollback and native-hide reclaim/restore path; clear remains authoritative across page, renderer, and retained output. | `2eee0e9`; memory policy tests; `pnpm check`. | WebView2 private-bytes curve during hide/show, clear, and pane destruction. |
| Host attach/resize | Attach progress is non-blocking; actual pane geometry is claimed and resize acknowledgements carry workspace identity. | `hosts.ts`, `paneSizeSync.ts`, `remote_host_impl.rs`; host/remote tests. | Physical dual-Host drag and resize. |
| Commune/mobile warning | Commune desktop/mobile capability paths and one Git-pill render site verified. Project source has no Chrome Extension Messaging; `runtime.lastError` remains environment attribution work. | source/bundle audit; capability/UI tests. | Phone source URL plus clean-profile and extension A/B. |
| Multi-window singleton | Independent desktop windows; Remote workspace ownership remains global with focus handoff. | ownership/race tests and existing contract. | Physical focus/close race. |

## Quality gates

- `pnpm test -- --reporter=dot`: 117 files, 1360 passed, 1 skipped.
- `pnpm check`: 0 errors, 0 warnings.
- `pnpm exec vitest run packages/remote/src/shared/transport --reporter=dot`: 15 files, 192 passed; one malformed-frame warning is the expected negative fixture.
- `cargo test --manifest-path src-tauri/Cargo.toml remote_host_impl::jsonrpc_tests --no-fail-fast`: 8 passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: exit 0; existing warnings only.
- `node scripts/check-release-version.mjs`: `0.1.24` contract OK.

## Release gate

Do not mark release complete until `gh release view v0.1.24` lists matching Windows, Linux, and macOS installer/CLI assets, Remote publish reports both desktop/mobile indexes, and ridge-cloud health reports the deployed revision. External checks are sampled at least five minutes apart.

## Carry-over and next iteration

The physical phone warning, public WebRTC/TURN measurements, WebView2 long-run memory curve, dual-window/dual-host physical runs, and Agent/headless real-process evidence remain explicit carry-over. The next Agent's Commune intake must pass the Pending approval gate before NLM cold-loop research or business-code changes begin.
