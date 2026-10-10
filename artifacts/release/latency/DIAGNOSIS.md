# CHG-067 latency diagnosis — 2026-10-10

## Result and scope

Local diagnosis delivered; production responsiveness is **NOT ACCEPTED** by this report. No transport, batching, capacity, retry, authentication or byte-order optimization was made. The available evidence prioritizes history/snapshot/control scheduling for further investigation; it does not establish the cause of the user's WAN symptom.

## Topology and identity

Owned Windows debug CLI host, separate owned Kernel, fresh headed Chromium controller, authenticated TLS WebSocket business framing, and real keyboard input to Python / PowerShell / cmd. CLI-to-Kernel writes use HTTP; optional RTP1 concerns Kernel output and is not evidence of WebRTC or faster input. WebGPU was reported by the controller. `visibleText` observes parser/grid state, not physical screen presentation.

All selected runs used binary SHA256 `86e13ed2f9d4000ae27e8f50a7639f4ba27887ae508fed75d5f74f54fa3f9456`, source HEAD `310c57956af7d269aa32e7010a8e5e40b2e976c6`, frontend index SHA256 `3350634140e19fda53884d23299cce27e0167bb5c67b90cc530e8737f4a64bdc`. Raw JSON records dirty-source fingerprints separately, loaded JS/WASM/CSS paths and hashes, flags, service-worker registration, and observed worker creation. No dedicated workers were observed; this cannot prove worker execution. These fingerprints identify the fixture, not the installed production executable. Production binary/version remains NOT_COVERED.

Node `performance.now()` supplies round trips. Browser spans and Rust `Instant` spans remain separate clocks. Server `receivedAtNodeMs` means log receipt, never server execution time. Percentiles below exclude unsuccessful samples; attempts and failures are shown separately. Four samples are exploratory: P95/P99 equal the maximum and are not stable tail estimates.

## Local real-reply samples

Milliseconds, `success/attempts`, P50, P95 = P99 = max. Off uses `verified-off-bench.json`; on uses `ownership-fixed-on-bench.json`, except switching uses the subsequent `owned-switch-on-bench.json` with verified workspace ownership. Every listed reply was absent from submitted literal input.

| Scene | Trace off | Client trace on |
| --- | --- | --- |
| Idle | 4/4; 27.60; 45.38 | 4/4; 29.66; 31.67 |
| Sustained output | 4/4; 14.41; 28.55 | 4/4; 15.07; 18.30 |
| First open, after child ready | 4/4; 14.23; 38.51 | 4/4; 29.60; 34.99 |
| Switch back | 4/4; 36.59; 43.66 | 4/4; 34.06; 44.79 |
| History output + wheel | 4/4; 287.04; 1509.75 | 4/4; 321.70; 1602.69 |
| Reload/reconnect + reply | 4/4; 426.88; 568.45 | 4/4; 375.71; 488.80 |
| PowerShell | 4/4; 35.76; 42.03 | 4/4; 34.02; 59.48 |
| cmd | 4/4; 34.45; 36.30 | 4/4; 37.84; 38.32 |

Startup/authentication/child installation are outside reply samples; transition times are separate in JSON. Input timing begins immediately before Enter, excluding nonce typing. Trace-on and off were sequential and the harness was corrected between runs. Typecheck overlapped the beginning of `ownership-fixed-on`; therefore no reliable probe-overhead percentage or speedup follows from this comparison. Server tracing and optional event-loop sampling were off in the selected client-on runs.

Direct Node Kernel child RTT in the off run: 4/4, P50 15.64 ms, P95/max 30.81 ms. Required live Rust Kernel waterfall: 30/30 replies, write P50/P95 1247/1725 us, poll-to-first-read 1127/14319 us, total from write start 2288/15873 us. Other raw runs also report 30 successful Rust replies. This is a distinct client and fixture workload; its values cannot be subtracted from UI RTT to obtain rendering or transport cost.

Existing `get_active_workspace_id` application read uses authenticated business framing but a control worker and full Kernel snapshot, while PTY input has a dedicated worker/direct path. Its roughly 4–7 ms successful RTTs are a control-path comparison, not a same-lane no-PTY echo or proof that PTY transport has that cost.

## Ranked findings

1. **CONFIRMED — historical test could misclassify evidence.** `PHASE-A6-LIVE-VALIDATION.md:35` reports about five-second P95; the old waterfall did not require every marker, could accept terminal echo, timestamped first data before the poll completed, and excluded write time from total. `packages/ridge-kernel/tests/kernel_backend_waterfall.rs` now uses a distinct child reply, fragmented-frame tail matching, post-read timestamps and 30 required successes. Corrected local runs have total P95 about 16 ms. This invalidates the old scheduler attribution as an established conclusion; it does not prove all historical five-second samples were artifacts. The earlier 110–160 ms bash/curl explanation likewise lacks a matched original raw baseline here.
2. **CONFIRMED — control queue stalls exist in the traced fixture.** `verified-on-bench.json` retains 538 dispatch records: input 60 records, queue wait P50/P95/max 38/57/2991 us; control 478 records, 48/777140/784597 us. Queue depth reached 16 and queued wire bytes 2160. The same bounded tail contains PTY writer lock wait P95 0 us, max 1 us; write/flush P95 16 us, max 19 us (54 records). These are same-process spans, not summed costs. Tail truncation and incomplete per-input correlation prevent assigning control stalls to an individual UI timeout.
3. **CONFIRMED — history workload has a larger observed reply tail.** Both selected runs reached about 1.5–1.6 seconds maximum, versus tens of milliseconds for idle. History output, viewport scrolling and observation are combined; this is not isolated parser/render time.
4. **HYPOTHESIS — snapshot/control scheduling or history processing merits the next reproduction.** Source anchors: `packages/ridge-cli/src/kernel_host_impl.rs:889` dispatch wait, `:1233` direct input, `:1308` snapshot-backed application read; `packages/ridge-kernel/src/pty.rs:1253` writer span; `packages/remote/src/shared/transport/paneRpcScheduler.ts:481` input RPC/admission age. The data supports inspecting these routes before changing transport. It does not establish snapshot, HTTP, PTY lock or encryption as the WAN bottleneck.
5. **CONFIRMED — prior harness selection produced invalid negative samples.** Earlier attempts used arbitrary workspace tabs or shell panes without confirming a controlled reply process. The final harness focuses the exact pane and stores verified child/workspace ownership; invalid selections fail as NOT_COVERED. `final-client-on` history/reconnect negatives and early switching timeouts are excluded from performance conclusions. Bounded timeouts are not recorded as successful latency.

## Probes and limits

`remotePerfTrace.ts:82` retains at most 256 copied, whitelisted metadata samples, counts drops, and does no trace clock/stat collection while off. `:162` resolves selected candidate pair through transport ID, then explicitly selected fallback; nomination alone is insufficient. Channel ID correlates counters; multiple ambiguous channels are not combined. No addresses or payloads are retained. `priorityFrameQueue.ts` records admission age, unknown pre-enable age, byte/depth and flow/overflow counters while retaining existing ordering/capacity. Provider call sites pass actual channels.

Host dispatch queues and Kernel writer spans are opt-in. Source IDs are bounded identifiers (`kernel_host_impl.rs:94`), not arbitrary log data. Server capture retains 2048 metadata records; the exploratory all-trace run dropped 4650. Client history trace retained 256 records and dropped 719; first-open dropped 416. Thus full event reconstruction and rate attribution are unavailable from these truncated tails. Queue rates may be computed only over retained records within their own lifetime, not asserted as whole-run throughput. Lease waits and long polls include waiting for data, not necessarily busy work: retained poll P95 about 51.7 ms/max 813 ms cannot be classified as CPU stalls.

## Coverage gaps and next bounded change

| Scenario / attribution | Status and exact missing resource |
| --- | --- |
| Separate LAN controller | NOT_COVERED: no separate authenticated device/controller supplied |
| Actual WAN/cellular/WebRTC | NOT_COVERED: no authenticated remote controller/session supplied; no selected candidate pair to measure |
| Host WebView foreground/background/minimized | NOT_COVERED: owned CLI fixture has no host WebView; requires an owned desktop fixture with permission to change visibility |
| Real TUI | NOT_COVERED: no supplied TUI fixture; nvim/vim absent from PATH |
| Same-input-lane no-PTY echo | NOT_COVERED: existing harmless read uses control/snapshot route; no diagnostic RPC was added |
| Physical presentation / actual worker execution | NOT_COVERED: grid observations and worker-creation metadata do not measure these stages |
| End-to-end encryption, parse, feed, output/subscription correlation | PARTIAL: existing traces/source audit, no complete untruncated per-input causal trace across all stages |

Smallest justified follow-up: on the actual authenticated symptom route, reproduce history/switch/reconnect with verified pane ownership, capture a bounded per-input window including control/input queue spans and output sequence, and collect client event-loop/parse/render-submission spans. Keep network, batching, capacities, encryption order and input retry policy unchanged until the cause is correlated. Regression criteria: every controlled reply is required; report timeouts separately; preserve FIFO/dependent delta order, authentication and cross-workspace protection; run the same build/workload with probes off/on; collect substantially more than four samples for tail/overhead conclusions. A production optimization requires its own approved change.

## Reproduction and verification

Build with `cargo build -p ridge-cli --bin ridge`, `npm run build:remote:desktop`; compile the ignored waterfall with `cargo test -p ridge-kernel --test kernel_backend_waterfall --no-run`. Set `RIDGE_BIN`, `RIDGE_WATERFALL_TEST` to those exact executables, choose an available `RIDGE_BENCH_HOST_PORT`, then run `node scripts/bench-latency-4scenes.mjs` with `RIDGE_BENCH_SAMPLES=4`, `RIDGE_BENCH_SCENES=idle,heavy,firstopen,switchback,history,reconnect,powershell,cmd,tui`. `RIDGE_BENCH_TRACE=1` enables client metadata; server and loop tracing have separate flags. `--help` exits before fixture side effects. JSON records executable/frontend hashes; rebuilding creates a new identity.

Executed: `npm test` (2129 passed, 17 skipped, plus 6 walker tests); `npm run lint`; `npm run typecheck` (both zero errors, one existing `RidgePane.svelte:70` warning); relevant Kernel/CLI/TLS Rust tests; live required 30-reply waterfall; real headed authenticated fixture runs; `node --check`; `git diff --check` with CRLF-aware whitespace configuration; SpecTree verification/completion recorded in repository state. Unit/build success does not imply WAN performance acceptance.

Changed source: remote metadata recorder/tests, priority queue/tests, provider channel correlation, input scheduler spans, read-only E2E hooks, CLI dispatch/input metadata/tests, Kernel writer spans/live waterfall, test-only absolute TLS directory override/tests, and isolated benchmark. Authoritative admission and approval/lock state accompany the source.

Fixtures use separate auth/TLS/Kernel directories and scoped browser leaf-SPKI pins. No system trust or installed-service state was changed. Owned Kernels shut down through authenticated `/v1/shutdown` (HTTP 200 in selected runs). Temporary `%TEMP%/ridge-diag-*` directories remain: automated recursive cleanup was rejected with `blocked by policy`; no alternate deletion was attempted. Auth files/private keys and executable binaries are excluded from committed evidence.
