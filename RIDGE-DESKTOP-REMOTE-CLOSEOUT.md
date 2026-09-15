# RIDGE-DESKTOP-REMOTE-CLOSEOUT

> Status: **GOAL_PARTIAL**
> Date: 2026-09-15
> Last commit: `df174ee7 docs(current-state)` + uncommitted `feat(desktop) wire Rtp1OutboundTransport`

This document records what is wired, what is not, and the exact
reproduction commands. It does NOT declare production readiness.

---

## 1. Actual production call chains (current state)

| Surface | Path | Protocol | Verified |
|---|---|---|---|
| `ridge` CLI → local kernel | `start_subscription` (default) | HTTP `/v1/domain/ptys/:id/output` long-poll | YES (kernel 157 PASS, CLI 175 PASS, live RTP1 WS e2e 1 PASS) |
| `ridge` CLI → local kernel | `start_subscription_rtp1` (`RIDGE_RTP1_KERNEL=1`) | RTP1 WS `/v1/rtp1` | Yes (kernel 157 PASS, live RTP1 e2e 1 PASS) |
| Desktop Tauri → local kernel (local pane) | `commands::terminal::ensure_pane_pty_workspace_with_initial_size` → `install_kernel_pty` | HTTP `/v1/domain/ptys` (legacy adapter) | Yes (kernel 157 PASS) |
| Desktop Tauri → remote host (LAN, legacy rdg mux) | `OutboundClient` over rdg mux WS | mux 0x10 PANE_RAW | Yes (kernel 157 PASS, mock tests PASS) |
| Desktop Tauri → remote host (RTP1, NEW v9-4 wiring) | `bind_rtp1_outbound_and_list` → `store_rtp1_transport` → `OutboundClient` wrapping `Rtp1OutboundTransport` | RTP1 WS (placeholder) | **NO** — never called by any production path; `rtp1_bind_outbound_and_list` is a `#[tauri::command]` not wired to the frontend connect button |

---

## 2. Fixes this session

### 2.1 Confirmed existing (no change)

* **RTP1 header = 11 bytes** verified against `packages/ridge-kernel/src/rtp1.rs:156` `HEADER_LEN = 4 + 1 + 1 + 1 + 4`. Matches SPEC §3.3.
* **2 pre-existing test failures** investigated:
  * `pty::tests::interactive_bridge_delivers_input_to_child` — ENV_BLOCKED. Spawns `sh` / `powershell` and expects shell echo. In sandbox the shell output is buffered or hidden; depends on real PTY environment. Pre-existing.
  * `commands::terminal::pty_lifecycle_contract_tests::restart_reattach_replays_bounded_kernel_history_and_reports_orphans` — static-assertion drift. Greps for `after_seq: None` and `orphaned += 1` in the source. The production code path moved and the strings are gone. Pre-existing static-assertion outdated.
  Both fail **independently of any current-session changes**. Do NOT loosen assertions.

### 2.2 New work this session

* `Rtp1OutboundTransport` rewritten to use real `read_domain_remote_hosts` + `running_endpoint` (v9-4 placeholder replaced).
* `bind_rtp1_outbound_and_list` (async, takes `&AppState`) + `rtp1_bind_outbound_and_list` (Tauri command).
* `HostRegistry::store_rtp1_transport` added (persists the legacy `OutboundClient` shape so the existing pipeline still works; the rtp1 client itself is read directly by the desktop Tauri app via its own `Rtp1KernelClient`).
* `RIDGE-CURRENT-STATE.md` generated and committed at `df174ee7`.
* Local `cargo build -p ridge`: PASS (warnings only — unused fields/types in `Rtp1OutboundTransport` because the live read path is not wired yet).

---

## 3. Reproduction commands

```bash
# Kernel unit + integration tests
cargo test -p ridge-kernel 2>&1 | grep "test result" | head -10
# → 157 PASS, 1 FAIL (interactive_bridge, pre-existing)

# CLI unit + live RTP1 e2e (this DOES exercise the rtp1 wire)
cargo test -p ridge-cli --bin ridge 2>&1 | grep "test result" | head -3
cargo test -p ridge-cli --test rtp1_kernel_e2e 2>&1 | grep "test result"
# → 175 PASS, 1 PASS

# Ridge (Tauri) lib — 270 PASS + 2 pre-existing FAIL
cargo test -p ridge --lib 2>&1 | tail -3

# Build check (clean)
cargo build -p ridge 2>&1 | grep "error\[" | head -3
# → empty (warnings only)
```

---

## 4. Old vs new path performance comparison

Not measured for the rtp1 path. The new `Rtp1OutboundTransport` is wired but never called by any production UI flow; no `attach / input / output / resize / detach` events have been issued through it. The only rtp1-end-to-end signal we have is `scripts/rtp1-kernel-e2e.mjs` which exercises the kernel's `/v1/rtp1` endpoint directly via `Rtp1KernelClient` — that path covers the rtp1 envelope + attach_ack + input_ack + detach_ack, not the desktop Tauri panel wiring.

| Metric | Status | Source |
|---|---|---|
| rtp1_attach_latency (in-process) | p50=3 µs, p95=5 µs, p99=10 µs | `performance_baseline` |
| pty_output_throughput | 186 MiB/s | `performance_baseline` |
| input → output (in-process) | p50=6 µs, p95=12 µs, p99=49 µs | `performance_baseline` |
| input_ui_to_render_submit (desktop) | **UNMEASURED** | requires Tauri GUI runner |
| Desktop Remote panel → Headless Host e2e | **UNMEASURED** | wiring incomplete |

---

## 5. Regression status

| Path | Result |
|---|---|
| Kernel unit + integration | 157 PASS, 1 FAIL (pre-existing ENV_BLOCKED) |
| CLI unit | 175 PASS |
| Live RTP1 WS e2e (kernel endpoint) | 1 PASS |
| Ridge (Tauri) lib | 270 PASS, 2 FAIL (pre-existing static-assertion drift) |
| Desktop local terminal | PASS (kernel integration via HTTP lease) |
| Desktop Remote panel (LAN, rdg mux) | PASS (legacy path) |
| Desktop Remote panel (RTP1, new path) | **NOT WIRED** — no production call site |
| Cloud Remote | OUT OF SCOPE for this repo (ridge-cloud lives elsewhere); the desktop Tauri app has Cloud subs/panel that we did not regress this session |
| Headless (ridge remote --daemon) | PASS (uses Rtp1KernelClient when RIDGE_RTP1_KERNEL=1, otherwise legacy HTTP) |

---

## 6. Default entry, kept compat entries, blockers

### Default entry

* `ridge` CLI default = legacy `start_subscription` (HTTP long-poll over `/v1/domain/ptys/:id/output`). Production-validated.
* `ridge` CLI with `RIDGE_RTP1_KERNEL=1` = `start_subscription_rtp1` (RTP1 WS over `/v1/rtp1`). Same-connection-level coverage, code-validated.
* Desktop Tauri default = legacy `start_subscription` (HTTP long-poll). Production-validated.

### Kept for compatibility

* `OutboundClient` (rdg mux): LAN Remote legacy path. Keep running.
* `MockOutboundTransport` (tests only): keep for unit tests of OutboundClient.

### Blockers

1. **Desktop Tauri UI button wiring** — the new `bind_rtp1_outbound_and_list` is exposed as a `#[tauri::command]` but no frontend code invokes it. The Hosts panel / connect flow does not consume it yet.
2. **Live read loop** — `Rtp1OutboundTransport::rtp1_client` was deliberately dropped (the desktop app reads `output` frames via its own `Rtp1KernelClient`). A live refresh loop inside the transport is not implemented and not required by the current pipeline.
3. **`input_ui_to_render_submit` P95 ≤ 16 ms** — not measured. Requires Tauri GUI runner.
4. **Headless exit / lifecycle regression test** — `restart_reattach_replays_bounded_kernel_history_and_reports_orphans` still asserts against a static source string; the production code path has moved. Need to replace the assertion or fix the source comment that the assertion greps for.
5. **`interactive_bridge` PTY echo** — sandbox blocks the underlying shell. Need a real PTY environment or a controlled echo test.

---

## 7. Reproducing the gap (raw commands)

```bash
# 1. Start kernel
target/debug/ridge kernel ensure &
sleep 1
cat $LOCALAPPDATA/ridge/kernel.json   # or wherever RIDGE_KERNEL_DATA_DIR points

# 2. RIDGE_RTP1_KERNEL=1 → rdg side picks RTP1 path
RIDGE_RTP1_KERNEL=1 target/debug/ridge remote --daemon

# 3. Desktop connect button → bind_rtp1_outbound_and_list is NOT invoked
#    from any UI flow; no real e2e through the desktop panel exists.

# 4. CI workflow exists: .github/workflows/rtp1-kernel-e2e.yml
#    but only runs the kernel-endpoint mjs harness, not the desktop
#    panel wiring.

# 5. Tests that still fail (pre-existing, do NOT loosen):
cargo test -p ridge-kernel --lib interactive_bridge_delivers_input_to_child
cargo test -p ridge --lib pty_lifecycle_contract_tests::restart_reattach
```

---

## 8. Output

**GOAL_PARTIAL**

What is verified end-to-end:
* rtp1 wire envelope correctness (kernel ↔ rdg client).
* rtp1 kernel endpoint attach / input / output / detach round-trip (`rtp1-kernel-e2e.mjs`).
* Old local terminal path no regression.

What is NOT verified end-to-end:
* Desktop Remote panel → Headless Host session list / attach / input / output / resize / detach / reconnect through the production UI path.
* kernel restart stale epoch rejection behavior.
* Desktop Remote panel input_ui_to_render_submit P95.

Reproduction commands are listed above. The new `Rtp1OutboundTransport` is built and compiles but is not wired to any production UI flow; the `rtp1_bind_outbound_and_list` command exists as a Tauri surface only.

DO NOT declare GOAL_COMPLETE. DO NOT declare BETA_READY. DO NOT remove the legacy `OutboundClient` until the rtp1 path is fully integrated through the desktop UI.
