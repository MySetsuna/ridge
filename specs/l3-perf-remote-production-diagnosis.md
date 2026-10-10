---
id: L3-PERF-REMOTE-PRODUCTION-DIAGNOSIS
level: L3
title: Authenticated production latency attribution
status: LOCKED
lifecycle: ACTIVE
parent: L2-PERF-001
code_targets:
  - packages/remote/src/shared/transport/remotePerfTrace.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.ts
  - packages/remote/src/shared/transport/wsRemote.ts
  - packages/remote/src/shared/cloud/controllerCloudProvider.ts
  - packages/remote/src/shared/cloud/ridgeCloudProvider.ts
  - packages/remote/src/shared/cloud/cloudHostBridge.ts
  - packages/remote/src/shared/cloud/priorityFrameQueue.ts
  - packages/remote/src/shared/terminal/manager.ts
  - src/remote/MainApp.svelte
  - src/remote/lib/paneFeedScheduler.ts
  - src-tauri/src/commands/cloud_pane.rs
  - src-tauri/src/commands/terminal.rs
  - src-tauri/src/remote_host_impl.rs
  - src-tauri/src/state.rs
  - src-tauri/src/engine/kernel_pty.rs
  - src-tauri/src/engine/pty.rs
  - packages/ridge-cli/src/kernel_host_impl.rs
  - packages/ridge-kernel/src/pty.rs
  - packages/ridge-kernel/src/client.rs
  - scripts/bench-latency-4scenes.mjs
  - packages/ridge-remote/src/tls.rs
test_targets:
  - packages/remote/src/shared/transport/remotePerfTrace.test.ts
  - packages/remote/src/shared/transport/paneRpcScheduler.test.ts
  - packages/remote/src/shared/cloud/priorityFrameQueue.test.ts
  - packages/remote/src/shared/cloud/cloudHostBridge.test.ts
  - packages/ridge-kernel/tests/kernel_backend_waterfall.rs
---

# Authenticated production latency attribution

## Contract

Diagnosis must measure the route actually invoked by the authenticated UI. Kernel remains the sole PTY/session authority. Reuse the existing opt-in Remote performance trace, performance measures, scheduler diagnostics and Rust tracing. Probes are disabled by default, bounded, metadata-only and removable. They may not change transport selection, batching, queue capacities, input retry policy, authentication, encryption, PTY byte order or dependent delta order. Behavioral experiments require a separate approved change after measurement.

Record source HEAD and dirty-state fingerprint separately from installed binary hash/version, frontend artifact hash, loaded resource URLs, active service-worker version, flags and observed transport. Source analysis alone cannot establish which build runs. Distinguish local Kernel HTTP and optional RTP1-over-WS from WAN WebRTC; distinguish signaling from payload, raw bytes from semantic deltas and declared interfaces from invoked UI paths.

## Measurements

Compare three layers on the same authenticated route: a small bounded application echo through existing business framing, queues and encryption without PTY; a controlled Kernel PTY child reply without rendering; and real UI input through parsing and render submission. Reuse an existing harmless request/response if it traverses the same route; any diagnostic echo surface requires explicit admission after authentication, size limits and no privilege expansion. Child replies use responses absent from the submitted input, so terminal-driver echo cannot satisfy the measurement. Also cover real available shells and TUIs. Exclude startup and authentication from steady-state samples.

Use client monotonic round trips and same-process monotonic spans only. Correlate existing RPC IDs, input source/sequence, pane activation and output sequence where present; never subtract unsynchronized host/client clocks. Instrument input capture, host receipt, PTY lock/write, output read, publish, enqueue/dequeue, send, client receipt, parse completion and render submission. Render submission is not physical presentation. Bound and explicitly report trace truncation and probe overhead.

Measure each encountered queue's bytes, depth, oldest-item age, enqueue/dequeue rates and overflow/resync counts. Attribute blocking lock waits, async scheduling and event-loop gaps, poll rearm/wakeup, feed/parser costs and render scheduling. Audit sequential RPC/ACK waits, connection reuse, batch timers, serialization/copies, encryption execution/order, duplicate subscriptions/raw-plus-delta delivery and cross-pane interference. Verify whether input or echo waits behind history, snapshots, resize ACKs, activation or reconnect. Verify actual worker execution and host WebView foreground/background/minimized behavior with traces.

For WebRTC resolve the selected candidate pair from transport.selectedCandidatePairId; record both candidate types, protocol/relay protocol, candidate-pair RTT statistics, channel ordering/reliability, bufferedAmount and application echo RTT. Candidate-pair STUN RTT is not application latency; relay use alone is not evidence of a bottleneck. Avoid saving credentials, candidate addresses or terminal payloads.

## Evidence and acceptance

For idle typing, sustained output plus typing, switching, history loading, reconnect and host foreground/background, record attempted/successful sample counts, P50/P95/P99/max, timeouts and raw distributions. Cover accessible localhost/LAN and the actual WAN/cellular route. Mark inaccessible scenarios NOT_COVERED with the precise missing resource. Network emulation is supplementary, isolated and explicitly labeled; no system networking changes or newly exposed ports. Recheck the historical 110–160 ms harness artifact and approximately five-second P95 without assuming HTTP or scheduler blame.

Unit tests validate opt-in behavior, bounded metadata retention, accurate selected-pair resolution, queue counters/age, overflow visibility and unchanged ordering/authentication. They do not prove production responsiveness. Run npm test, npm run lint and npm run typecheck; run relevant Rust checks when Rust probes change. Save one diagnosis report with topology and file:line anchors, reproducible commands, raw evidence paths, coverage gaps, ranked CONFIRMED/HYPOTHESIS findings, measured stall contributions, smallest evidence-justified next change and regression criteria. List executed tests, changed files and uncertainties. Never declare overall performance PASS from partial coverage.

## Isolated fixture admission

The existing benchmark runs only an owned test host and Kernel. A TLS directory override is admitted only with the existing test-fixture permission flag, requires an absolute path and fails closed for malformed configuration. Without that opt-in, the production TLS directory and certificate policy are unchanged. The fixture uses its own auth file and per-browser leaf SPKI pin, without changing system trust or the installed service certificate files. Cleanup uses the authenticated Kernel shutdown endpoint; retained temporary state must be disclosed.
