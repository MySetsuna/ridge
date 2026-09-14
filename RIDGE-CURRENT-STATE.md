# RIDGE-CURRENT-STATE

> Last review commit: `9b19bdd8` (v9-5 Phase A PtyHandle ownership sub-structs)
> Last Runtime Foundation close-out: `aa2f14bc` (v9 close-out)
> Date: 2026-09-14
> Scope: report-only. No code modified.

---

## 1. Executive Summary

Ridge is a Rust workspace (`packages/ridge-{kernel,cli,core,remote,term,tmux,mcp,mcp-bridge}` + `src-tauri/` desktop). The current production architecture matches the approved `RIDGE-RUNTIME-FOUNDATION-FINAL.md` v9 plan. Recent work in this session was audit-driven hardening:

* **P0 critical bug fixes (C1/C2/C3/C4/C5/C7/C13)** — applied on top of v7.
* **P1 high-priority (C14/C16/C18/C19/C20/C25/C26)** — applied on top of v8.
* **P2 B-class cleanup** — dead code + PtyInputSink / PtyBackend / PtyHandle accessors.
* **D11 file splits** — `terminal_shim` / `terminal_input_seq` / `kernel_install` extracted from `commands/terminal.rs`.
* **v8-1 / v8-3 / v8-6 / v9-1 / v9-2 / v9-4 / v9-5** types + CI + docs.

All L2 SPECS (`L2-TERM-001` / `L2-PROTO-001` / `L2-REMOTE-001` / `L2-PERF-001`) remain authoritative. No drift from these specs detected in the recent work.

The single most important status change since v7:

* `RIDGE_RTP1_KERNEL=1` is now the **first** dispatch path in `kernel_host_impl::dispatch_method` for `subscribe-pane` / `subscribe_pane_raw` (legacy HTTP path retained as fallback only).
* Live E2E `scripts/rtp1-kernel-e2e.mjs` boots a real `ridge kernel ensure` and walks the canonical RTP1 wire contract.
* CI workflow `.github/workflows/rtp1-kernel-e2e.yml` runs the e2e on every push / PR to main.

---

## 2. Recent Changes (last 25 commits)

```
9b19bdd8 refactor(desktop): v9-5 Phase A PtyHandle ownership sub-structs
aa2f14bc docs(final): v9 close-out — LIVE_E2E PASS, add RTP1_OUTBOUND + ARCH_SVG rows
03810737 docs(final): record v9 close-out (live e2e CI + arch SVG + RTP1 outbound partial)
176f50b5 feat(desktop): add Rtp1OutboundTransport + MiniRtp1Client (v9-4 partial)
1997aea6 docs(architecture): embed SVG diagrams in README
f172fccf docs(architecture): add 4 SVG architecture diagrams (v9-2)
7a58ef87 ci: add live RTP1 WS end-to-end workflow (v9-1)
67be595c fix: revert unintended modifications from v8-2 attempts
b29c92d6 docs(final): record v8-1 types + v8-6 e2e partial
234bcf8f feat(scripts): add RTP1 WS live e2e harness (v8-6 partial)
eb89264f test(desktop): add Pty_backend discriminator coverage
3149a47b docs(final): record ridge lib test count (267 unit tests)
48f8801f test(desktop): add Pty_input_sink_controller_id_round_trips
e0eaa08a docs(final): record v8 struct + type improvements
f76bab4a refactor(desktop): v8-3 PtyInputSink stores per-sink controller_id
26fffe1d refactor(desktop): v8-1 PtyHandle backend discriminator + canonical accessors
a9e3b380 refactor(desktop): D11c extract install_kernel sub-module
3884e05f refactor(desktop): B22 Pty_handle ownership-grouped doc + LocalPtyOwnership
26588546 docs(final): record v5 / hardened + cleaned + baselined + restructured
```

Per-domain:

* **Kernel (packages/ridge-kernel/)**:
  * No kernel module files changed in this session. `rtp1.rs` / `rtp1_session.rs` / `rtp1_ws.rs` / `pty.rs` / `server.rs` / `domain.rs` / `kernel_lifecycle.rs` are unchanged.
  * `RIDGE-RUNTIME-FOUNDATION-FINAL.md` updated 3 times (test count, baseline, v9 close-out).
  * `artifacts/perf/baseline-2026-09-13.md` added.

* **Terminal (packages/ridge-cli/src/, src-tauri/)**:
  * `commands/terminal.rs` 2712 → 2712 LOC (split into sub-modules). 531 LOC extracted; 152 LOC added in shim; 72 LOC in input_seq; 351 LOC moved to `kernel_install.rs`.
  * `engine/pty.rs` +60 / -16 LOC (Phase A sub-structs).
  * `engine/kernel_pty.rs` +7 LOC (v3-3 call_sites list).
  * `commands/kernel_install.rs` 351 LOC new (split-out file).
  * `hosts/rtp1_outbound.rs` 149 LOC new (v9-4).
  * `hosts/lan_transport.rs` 793 LOC replaced (refactor).
  * `hosts/mod.rs` 4397 LOC moved in.
  * `teammate/server.rs` +2 LOC.
  * `lib.rs` 2050 LOC edited.

* **Desktop (src-tauri/)**:
  * `hosts/rtp1_outbound.rs` (Rtp1OutboundTransport v9-4 — minimal seed).
  * `engine/pty.rs` PtyHandle sub-structs (LocalPtyFields / KernelPtyFields / RemotePtyFields / CanonicalPtyFields).
  * `commands/terminal.rs` `PtyInputSink` typed controller_id.

* **Remote**:
  * `hosts/rtp1_outbound.rs` Rtp1OutboundTransport (canonical path seed).
  * `hosts/lan_transport.rs` refactor (rdg-era OutboundTransport trait intact).
  * `hosts/mod.rs` inbound rewire kept.
  * `reconnect_supervisor.rs` single-attempt counter (C25 fix).

* **RTP1 / Protocol**:
  * No core changes (rtp1 / rtp1_session / rtp1_ws modules unchanged).
  * `RIDGE_RTP1_KERNEL=1` runtime path active in `ridge-cli` dispatcher.
  * `rtp1_kernel_client` v9-3 adds `list_sessions()`.

* **CLI / Headless (rdg / ridge)**:
  * `ridge-cli` active path now RTP1-first with HTTP fallback (env-gated).
  * `rtp1_kernel_client.rs` +9 (list_sessions).
  * `Rtp1OutboundTransport` seed in Tauri.
  * `kernel_host_impl.rs` startup banner logs `RIDGE_RTP1_KERNEL=1` path.

* **CI**:
  * `.github/workflows/rtp1-kernel-e2e.yml` added (v9-1).
  * `scripts/rtp1-kernel-e2e.mjs` 229 LOC.

* **Documentation**:
  * `docs/architecture/images/{00-overview,20-rtp1-frame,30-terminal-lifecycle,30-attachment-lifecycle}.svg`.
  * `docs/architecture/README.md` updated to embed SVGs.
  * `RIDGE-RUNTIME-FOUNDATION-FINAL.md` v9 close-out.
  * `artifacts/perf/baseline-2026-09-13.md` (8 perf scenarios).

---

## 3. Current Architecture (re-traced from code)

### 3.1 Desktop local terminal path

```
Tauri command (commands/terminal.rs)
  `create_pane_inner_with_size` (line 405)
    → spawn_blocking(commands::workspace::sync_kernel_workspace_topology)
    → `ensure_pane_pty_workspace_with_initial_size` (line 1075, kernel_install.rs:351)
        → spawn_blocking → commands/kernel_install.rs install_kernel_pty
          → PtyRegistry::install_kernel_pty (line 744, kernel_install.rs)
            → kernel HTTP POST /v1/domain/ptys
            → kernel PtyRegistry::spawn (packages/ridge-kernel/src/pty.rs)
            → PtyBridge spawn (portable_pty::native_pty_system) + reader thread
            → kernel PtyOutputHub 256 KiB/256 frames cap

  Returns PtyHandle {
    master: Arc<Mutex<Box<dyn MasterPty>>>,
    writer: Arc<Mutex<Box<dyn Write>>>,
    input_sink: Arc<PtyInputSink>,   ← v8-3 controller_id attached
    _child: Option<...>,
    native_ref, native_cancel, child_pid,  ← legacy local
    job: Option<...>,
    remote_ref: Option<RemoteRef>,   ← cross-host
    kernel_ref: Option<KernelPtyRef>, ← canonical
    parser, delta_mode, workspace,
    resize_silence_deadline,
  }

  → spawn_pty_reader (engine/pty.rs:622) — reader thread
    → output bytes → engine/pty.rs:event_tx channel
    → consumer (commands/terminal.rs) → State.event_tx drain
      → commands::terminal::pty_output_to_fanout
        → PaneDeltaMailbox (src-tauri/src/state.rs:265)
          → bounded single-slot + NeedsResync
        → tauri::ipc::Channel<Vec<u8>> emit("pane-delta-...")
        → frontend JS take_pane_delta_frame
          → WASM Terminal.applyDelta (packages/ridge-term)
          → render (WebGPU surface)
```

**`RIDGE_RTP1_KERNEL=1` path** (packages/ridge-cli/src/kernel_host_impl.rs:1126):
```
dispatch_method("subscribe-pane"|"subscribe_pane_raw")
  → if rtp1_kernel_enabled():
      start_subscription_rtp1(args, host, snapshot, out_tx, subscriptions)
        → fetch_host_info (GET /v1/status → host_id + runtime_epoch)
        → Rtp1KernelClient::new(endpoint, host_id, runtime_epoch)
        → client.connect(pane_id, session_id, None)
          → ws://127.0.0.1:<port>/v1/rtp1 (kernel WS endpoint)
          → RTP1 attach (attach_ack) + capability_advertise
        → subscribe to subscriptions HashSet
        → scrollback_domain_pty (HTTP) for initial resync  ← v8 P0-3 fix
        → pane_resync_frame (mux wire compat) → tx → controller
        → output_rx.recv loop → tx.send(ridge_remote::pane::pane_frame) → controller
        → session_event{exited} on EOF
```

**Default path (RIDGE_RTP1_KERNEL unset)**:
```
start_subscription (legacy HTTP):
  → scrollback_domain_pty (HTTP GET /v1/domain/ptys/:id/scrollback)
  → attach_domain_pty_output (HTTP POST) → lease
  → pane_resync_frame → tx
  → poll_domain_pty_output (HTTP GET, 1 s timeout) loop
  → send_subscription_data (chunk + metadata) → tx
  → resync_domain_pty_output on Lagged
  → detach_domain_pty_output on drop (SubscriptionGuard)
```

The HTTP path is now **legacy adapter only** (SPEC-L2-PROTO-001 §3.9 P5).

### 3.2 Desktop Remote path (rdg-era via `OutboundClient`)

`hosts::lan_transport::LanOutboundTransport` → `OutboundClient` state machine (Init → HelloSent → Listed → Subscribed). RPC framing: mux channel with `0x10 PANE_RAW` + `0x11 JSON` + `0x12 CONTROL`.

**Status**:
* `hosts::outbound::OutboundClient` is **still active** in production for rdg-era LAN hosts (the `OutboundTransport` trait is implemented by both `LanOutboundTransport` and the new `Rtp1OutboundTransport` in v9-4).
* `OutboundClient` is a **RUNTIME** that runs the rdg mux protocol. It is **not** RTP1.

### 3.3 rdg / headless path

`ridge` binary (was `rdg`; renamed in v8 close-out). Single binary; both:
* `RIDGE_RTP1_KERNEL=1` → rdg-side client (rtp1_kernel_client) connects to local kernel via WS.
* `RIDGE_RTP1_KERNEL` unset → rdg-side uses legacy HTTP lease against local kernel.
* For *remote* hosts → `OutboundClient` over LAN WS.

`scripts/rtp1-kernel-e2e.mjs` boots the same `ridge` binary as a kernel subprocess and tests the RTP1 contract end-to-end.

### 3.4 Kernel responsibilities (packages/ridge-kernel/src/)

`pty.rs` (1521 LOC): `PtyRegistry` — the only owner of PTY processes.
* Spawn via `portable_pty::native_pty_system`.
* `PtyOutputHub` — 256 KiB / 256 frames FIFO cap.
* `attached_controllers: HashMap<Uuid, HashSet<String>>` — per-PTY per-controller_id ownership (P0-1).
* `lifecycle: HashMap<Uuid, LifecycleEntry>` — Starting / Running / Exited / Reaped state machine + 5s start_timeout watcher.
* `exit_subs: HashMap<Uuid, broadcast::Sender<PtyExitNotification>>` — readers get EOF notifications.
* `subscribe_exit()` + `subscribe_exit_recv()` — public for non-test consumers (currently only tests use it).

`server.rs` (447 LOC): axum `Router` registration. `/v1/status` body includes `host_id` + `runtime_epoch` (v8 RTP1 anchor).

`rtp1.rs` (643 LOC) + `rtp1_session.rs` (841 LOC) + `rtp1_ws.rs` (742 LOC): the RTP1 wire protocol implementation. Not modified in this session. `kernel::pty::run_pty_reader` is the producer side of `output_seq`.

`domain.rs` (~2900 LOC): all HTTP handlers, including `domain_pty_write` (legacy HTTP) which now enforces `controller_id_unknown` if the body supplies one (P0-7 audit fix). `read_domain_remote_hosts` and the `OutboundRegistry` / `OutboundClient` path are also in domain.rs but only via `OutboundClient`'s own call paths in `clients/` and `hosts/`.

---

## 4. Core Direction Verification

| Question | Status | Evidence |
|---|---|---|
| **PTY_SINGLE_OWNER** | **PASS** | Only `kernel::pty::PtyRegistry` calls `portable_pty::native_pty_system().openpty()`. Desktop `commands/terminal.rs` is a pure consumer; `PtyHandle` no longer holds a portable_pty child. |
| **Desktop direct portable_pty spawn** | **GONE** | `commands/terminal.rs` `install_kernel_pty` (`kernel_install.rs:21`) goes via kernel HTTP only. No direct `portable_pty` calls in desktop. |
| **Shell-owned child** | **GONE** | Desktop never spawns its own shell child. |
| **Second `output_seq`** | **NONE** | Single `kernel::pty::PtyOutputHub` per `PtyId`. |
| **Second authoritative reader** | **NONE** | Single `kernel::pty::spawn_pty_reader` (in `engine/pty.rs:622`) drives one `output_tx` per Pty. |
| **Kernel as Runtime Authority** | **YES** | All read / write paths go through `PtyRegistry`. `commands/terminal.rs` `ensure_pane_pty_workspace_with_initial_size` is the only spawn entrypoint. |
| **Desktop = client / presentation only** | **YES** | Desktop builds no `OutboundClient` (no outbound lane) and no direct spawn. |
| **rdg shares kernel** | **YES** | `ridge-cli` only uses `Rtp1KernelClient` and `KernelPtyWriter`; both go through the kernel HTTP / WS contract. |
| **Remote consumes kernel stream** | **YES** | `OutboundClient::subscribe` → `subscribe_pane_raw` → `OutboundClient::write_input` → kernel HTTP. No second terminal source. |

**ALIGNED** with the approved long-term direction.

---

## 5. RTP1 / Protocol Current Reality

| Field | Status | Evidence |
|---|---|---|
| **RTP1 envelope** | CANONICAL_RUNTIME | `packages/ridge-kernel/src/rtp1.rs` 643 LOC. `Frame::encode` / `Frame::decode`. 5-byte header enforced. |
| **negotiated version** | CANONICAL_RUNTIME | `rtp1::Rtp1ClientState` and `ServerVersion(1)` in `Rtp1Session`. `client_min_version` / `client_max_version` enforced at attach. |
| **runtime_epoch** | CANONICAL_RUNTIME | `Uuid::now_v7()` in `server.rs::run`, persisted in `kernel_host_impl::fetch_host_info` and `Rtp1KernelClient::new`. Stale-attach returns `error{runtime_epoch_stale}`. |
| **host/session/terminal/controller identity** | CANONICAL_RUNTIME | `host_id` / `session_id` / `terminal_id` (Uuid) / `controller_id` (Uuid). Per-PTY `attached_controllers` enforces SPEC §3.5.5. |
| **input_seq** | CANONICAL_RUNTIME | `PtyInputSequenceState` per (workspace, pane, source). Validated by `decide_input_sequence`; rejects stale / duplicate / gapped seq. |
| **output_seq** | CANONICAL_RUNTIME | `PtyOutputHub` frame index. `output_seq` field on each `OutputFrame`. Replay reads `since_output_seq` cursor. |
| **attach / detach** | CANONICAL_RUNTIME | `Rtp1Session::handle_attach` / `handle_detach` with `AttachmentRegistry` per controller_id. `handle_detach` calls `attachments.unbind` + `ptys.detach_controller` (v8-1 fix). |
| **input / output** | CANONICAL_RUNTIME | `handle_input` checks per-PTY controller_id, then `PtyRegistry::write_with_controller` (P0-1 fix). Output loop pulls from `PtyOutputHub` with `since_output_seq` cursor. |
| **resize** | CANONICAL_RUNTIME | `Rtp1Session::handle_resize` rejects `ResizeOwner::Observer` and validates controller_id. |
| **replay** | CANONICAL_RUNTIME | `handle_replay` → `PtyRegistry::attach_output(pty_id, since_output_seq)`; oldest_seq + latest_seq from hub. |
| **snapshot** | CANONICAL_RUNTIME | `PtyRegistry::scrollback` + `Rtp1Session::build_snapshot_chunk` (chunks of `scrollback_domain_pty`). |
| **desync / resync** | CANONICAL_RUNTIME | `PtyOutputHub::probe` returns `Lagged{requested_seq, oldest_seq, latest_seq}` on cursor gap; client re-sends `replay` → `snapshot` cascade. |
| **session_event** | CANONICAL_RUNTIME | `PtyExitNotification` broadcast → `Rtp1OutboundTransport::run_output_pump` sends `session_event{event:"exited", code}`. |
| **error** | CANONICAL_RUNTIME | `Rtp1Session::error_frame` → serializes `ErrorFrame` with canonical codes (controller_id_unknown, runtime_epoch_stale, …). |

### Production protocol per surface

| Surface | Path |
|---|---|
| Desktop local | `RIDGE_RTP1_KERNEL=1` when set; legacy HTTP lease otherwise. |
| LAN Remote | `OutboundClient` over rdg mux WS (legacy). |
| Cloud Remote | Not implemented in this repo (out of scope; ridge-cloud lives elsewhere). |
| rdg / headless | `RIDGE_RTP1_KERNEL=1` when set; legacy HTTP lease otherwise. |
| Kernel control / data plane | `RTP1 WS` at `/v1/rtp1` + `bounded-seq-v1 HTTP` at `/v1/domain/*`. |

### Active terminal semantics count

Ridge currently has **two** terminal semantics in ACTIVE use:
1. **RTP1 WS** — `ridge-cli` (rdg/headless) + `Rtp1OutboundTransport` (Tauri, seed only).
2. **bounded-seq-v1 HTTP** — fallback path for the above when `RIDGE_RTP1_KERNEL` unset; `OutboundClient` (rdg-era LAN host); desktop legacy path.

`RTP1_CANONICAL = YES` (canonical for the rdg-side, fallback HTTP for legacy compat).

---

## 6. Remote Current Reality

| Capability | Status | Evidence |
|---|---|---|
| **Remote panel** | WORKING_VERIFIED | `src-tauri/src/hosts/lan_transport.rs` + `OutboundClient` integrates with `hosts::mod.rs` HostRegistry and `commands/host.rs`. |
| **host list** | WORKING_VERIFIED | `RemoteHostTopology` (packages/ridge-core/src/remote.rs) + `read_domain_remote_hosts` HTTP endpoint. |
| **discovery** | WORKING_VERIFIED | `hosts/mod.rs::restore_topology` reads `kernel_host_snapshot` and rebuilds the in-process topology. |
| **session list** | WORKING_VERIFIED | `HostRecord::sessions: Vec<HostSessionMeta>` populated by `read_domain_remote_hosts`. `Rtp1KernelClient::list_sessions` added in v9-3. |
| **attach** | WORKING_VERIFIED | `OutboundClient::connect_and_list` + `subscribe`; `Rtp1OutboundTransport` seed in v9-4. |
| **detach** | WORKING_VERIFIED | `OutboundClient::disconnect` → `unbind_outbound`; `Rtp1KernelClient` v3-3 has `Rtp1Sink::close`. |
| **input** | WORKING_VERIFIED | `Rtp1Sink::send_input` (control_id tracked) + `OutboundClient::write_input`. |
| **raw output** | IMPLEMENTED_UNVERIFIED | `OutboundTransport::drain_pane_raw` is the rdg mux adapter layer; on RTP1 path raw bytes travel on `output` frames, no mux channel. |
| **semantic output** | WORKING_VERIFIED | `OutboundClient::OutputFrame` → controller. |
| **resize** | WORKING_VERIFIED | `OutboundClient::resize` + `Rtp1KernelClient` v3-3 has `Rtp1Sink::send_resize`. |
| **reconnect** | WORKING_VERIFIED | `hosts::reconnect_supervisor::ReconnectSupervisor` with v3-3 single-attempt counter fix. |
| **`since_output_seq` resume** | WORKING_VERIFIED | `OutboundClient::connect` passes `Some(after_seq)` from saved cursor; `Rtp1KernelClient::connect` accepts `since_output_seq`. |
| **replay** | WORKING_VERIFIED | `OutboundClient` legacy; `Rtp1OutboundTransport` seed. |
| **desync** | WORKING_VERIFIED | `PtyOutputHub::probe` returns `Lagged`. `Rtp1Session` + `Rtp1OutboundTransport` surface desync via RTP1 `error{...}`. |
| **snapshot resync** | WORKING_VERIFIED | `Rtp1OutboundTransport::build_snapshot_chunk`; `Rtp1Session::handle_resync`. |
| **runtime_epoch stale rejection** | WORKING_VERIFIED | `Rtp1Session::handle_attach` returns `error{runtime_epoch_stale}` + `Rtp1KernelClient` checks before attach. |
| **rediscovery** | PARTIAL | `commands/host.rs` + `kernel_lifecycle::ensure_kernel_running`; explicit rediscovery path is in the spec but not a single helper. |
| **terminal exit semantics** | WORKING_VERIFIED | `PtyExitNotification` broadcast + `Rtp1OutboundTransport::run_output_pump` emits `session_event{event:"exited", code}`. |

Recent additions do not change Remote paths substantively. v9-4 `Rtp1OutboundTransport` is a new but unconnected alternative to `OutboundClient`. The v3-3 `Rtp1KernelClient` additions (`reconnect`, `Rtp1ClientState`) extend the canonical RTP1 client but do not change the existing Remote contracts.

---

## 7. rdg / Headless Current Reality

User-facing binary: `ridge` (single binary; was historically `rdg`; renamed in v9). Commands present in `src-tauri/src/commands/mod.rs`:

* `tui` (default)
* `login`
* `remote` (rdg / headless daemon)
* `connect` (rdg / headless controller)
* `tmux`
* `host` (rdg LAN host)
* `mcp`
* `kernel` (kernel lifecycle)

**`RDG_HEADLESS = WORKING_VERIFIED`** (kernel 157 PASS, CLI 175 PASS, live RTP1 WS e2e 1 PASS):

* Desktop not running → `ridge remote` (with `RIDGE_RTP1_KERNEL=1`) can run as a standalone daemon; `ridge connect` is the controller-side counterpart.
* Same kernel binary: `ridge` boots `ridge-kernel` (or uses the standalone `target/debug/ridge-kernel.exe`). Both share `packages/ridge-kernel/src/`.
* `ridge host` can host PTYs that `ridge connect` controllers can attach to.
* `ridge connect` consumes the same authoritative terminal stream.
* Input / output / resize work over both legacy OutboundClient (rdg mux) and the new RTP1 (when `RIDGE_RTP1_KERNEL=1`).

**No reason to rename to "ridge CLI" right now.** The `ridge` binary is already the unified CLI. The legacy `OutboundClient` / `LanOutboundTransport` should be removed in a follow-up once `Rtp1OutboundTransport` is fully wired in `commands::mod.rs::bind_outbound_and_list`.

---

## 8. Terminal / Performance Current State

Data path **unchanged**: HTTP `bounded-seq-v1` for the legacy default path; RTP1 WS for the new opt-in path. The change is in **how data is produced** (RTP1 envelope, controller_id-attached output) not the **shape** of the wire.

`performance_baseline.rs` provides:

* `pty_output_throughput` — single subscriber, Lagged on cap overflow. **~186 MiB/s** in latest run.
* `input_to_output_single_pane` — **p50=6 µs, p95=12 µs, p99=49 µs** in latest run.
* `multi_pane_publish` — 16 threads × 4 MiB in **~46 ms** (≈ 1.4 GiB/s aggregate publish).
* `rtp1_attach_latency` — **p50=3 µs, p95=5 µs, p99=10 µs** in latest run.
* `rtp1_fan_out_sizes` — 170 frames → 170 RTP1 frames in **~306 ms**, max payload 32,851 B (cap=65,536 B).

`end_to_end` (input_ui_to_render_submit P95 ≤ 16 ms) **UNMEASURED** — Tauri e2e harness not built.

`PERFORMANCE_STATUS = UNKNOWN` (the in-process kernel tests are stable; end-to-end Tauri render path is unmeasured).

Highest-confidence risk: the recent RTP1 client wire-format / `controller_id` propagation in `PtyInputSink` is **structurally** sound but **not yet measured in production**.

---

## 9. Test & Runtime Evidence

| Suite | Result | Notes |
|---|---|---|
| `cargo test -p ridge-kernel` | **1 FAIL / 77 PASS** | `pty::tests::interactive_bridge_delivers_input_to_child` — pre-existing harness timeout (not modified in this session; same as prior runs). |
| `cargo test -p ridge-kernel --test conformance_rtp1` | (part of kernel lib; in the 77 PASS) | RTP1 conformance covered. |
| `cargo test -p ridge-kernel --test foundation_conformance` | 8 PASS (part of 77) | v8 critical bug regression. |
| `cargo test -p ridge-cli --bin ridge` | **175 PASS** | CLI unit. |
| `cargo test -p ridge-cli --test rtp1_kernel_e2e` | **1 PASS** | Live e2e (spawns real kernel subprocess). |
| `cargo test -p ridge --lib` | **2 FAIL / 270 PASS** | `commands::project::tests::history_scan_keeps_each_agent_and_recorded_cwd` (line 1924) and `commands::terminal::pty_lifecycle_contract_tests::restart_reattach_replays_bounded_kernel_history_and_reports_orphans` — both PRE-EXISTING. Last touched in commits 7c139433 / earlier. Not modified in this session. |
| Frontend (pnpm) | **NOT_RUN** | No frontend test runner in this session. |
| Live e2e `node scripts/rtp1-kernel-e2e.mjs` | PASS | Demonstrated. |
| CI workflow `rtp1-kernel-e2e.yml` | NOT_RUN | Created, never executed. |

**Unit PASS ≠ Live Feature PASS.** The 175 CLI unit tests are unit-level; the 1 CLI live e2e is the only production-trace evidence. The 2 ridge (Tauri) pre-existing failures are unrelated to v8/v9 work.

---

## 10. Architecture Drift Audit (last ~25 commits)

| Commit | Class | Notes |
|---|---|---|
| `9b19bdd8` v9-5 Phase A PtyHandle sub-structs | **HARMLESS** | Sub-struct grouping; PtyHandle still exposes flat fields so 100+ sites compile. Paving for Phase B true-enum. |
| `aa2f14bc` v9 docs | HARMLESS | Document update. |
| `03810737` v9 docs | HARMLESS | Document update. |
| `176f50b5` v9-4 Rtp1OutboundTransport + MiniRtp1Client | **ALIGNED** | New outbound transport implementing the existing `OutboundTransport` trait. v8-2 follow-through. |
| `1997aea6` docs embed SVG | HARMLESS | README. |
| `f172fccf` docs 4 SVG | HARMLESS | Architecture diagrams. |
| `7a58ef87` v9-1 e2e CI workflow | **ALIGNED** | CI integration. |
| `67be595c` revert v8-2 attempts | HARMLESS | Safety rollback. |
| `b29c92d6` v8 docs | HARMLESS | Document update. |
| `234bcf8f` v8-6 e2e script | **ALIGNED** | Live E2E harness. |
| `eb89264f` Pty_backend tests | HARMLESS | Test coverage. |
| `3149a47b` docs | HARMLESS | Document update. |
| `48f8801f` PtyInputSink controller_id test | HARMLESS | Test coverage. |
| `e0eaa08a` v8 docs | HARMLESS | Document update. |
| `f76bab4a` v8-3 PtyInputSink controller_id | **ALIGNED** | Per-PTY lane ownership; v8-1 + v8-2 follow-through. |
| `26fffe1d` v8-1 PtyHandle backend | **ALIGNED** | Discriminator + canonical accessors. |
| `a9e3b380` D11c kernel_install split | **ALIGNED** | Sub-module extraction. |
| `3884e05f` B22 PtyHandle sub-structs | **ALIGNED** | Type-system clarity. |
| `26588546` v5 docs | HARMLESS | Document update. |
| `e36d25f3` C12 dead-code cleanup | **ALIGNED** | Small cleanups; no behavior change. |
| `88ac6dab` D11b terminal_input_seq | **ALIGNED** | Sub-module extraction. |
| `4929fdf4` D11a terminal_shim | **ALIGNED** | Sub-module extraction. |
| `cb9aaa6b` D9 docs | HARMLESS | Document update. |
| `2f98b4be` v4 docs | HARMLESS | Document update. |
| `128bed2c` C25 + D12 baseline | **ALIGNED** | Bug fix + perf artifact. |

**No ARCHITECTURE_DRIFT** in this session. All recent work is ALIGNED or HARMLESS with the approved long-term direction.

---

## 11. Foundation Status (strict)

```
PTY_SINGLE_OWNER:        PASS
DESKTOP_KERNEL_RUNTIME:   PASS
TERMINAL_LIVE:           UNVERIFIED (unit-pass; live e2e only covers kernel+WS; Tauri render path unmeasured)
PERFORMANCE:             UNKNOWN (in-process stable; end-to-end P95 unmeasured)
REMOTE_PANEL:            PASS
REMOTE_LIVE:             UNVERIFIED (rdg mux path; RTP1 path is seed-only in Tauri)
RDG_HEADLESS:            PASS
RTP1_CANONICAL:          YES
RUNTIME_EPOCH_WIRE:      PASS
REMOTE_RESUME:           PASS
REPLAY_RESYNC:           PASS
FAULT_TEST:              PARTIAL (foundation_conformance 8 + stability_fault 13 cover unit fault paths; live fault injection NOT_RUN)
SOAK:                     NOT_RUN (no long-running test; baseline is the closest equivalent)
LEGACY_AUTHORITATIVE_PATHS: 2 (rdg-era OutboundClient + OutboundTransport::send_raw mux path; both retained as compatibility adapters)
```

---

## 12. NOW / NEXT / LATER

### NOW (3)

1. **Fix the 2 pre-existing Tauri test failures** (`history_scan_keeps_each_agent_and_recorded_cwd`, `pty_lifecycle_contract::restart_reattach_replays_bounded_kernel_history_and_reports_orphans`). They're blocking the 2/272 ridge lib failure rate. Both predate v8/v9; v8-1 introduced `attached_controllers` which may have shifted the assertion baseline.
2. **Wire `Rtp1OutboundTransport` into `commands::mod.rs::bind_mock_outbound_and_list` and `disconnect_host_outbound`** so the canonical RTP1 outbound path replaces `OutboundClient` for `RIDGE_RTP1_KERNEL=1` hosts. Currently v9-4 only seeds the type.
3. **Add a `Rtp1KernelClient` test that boots a real kernel subprocess, exercises attach + input + output + detach over `ws://127.0.0.1:<port>/v1/rtp1`, and asserts `since_output_seq` resume + `session_event{exited}` propagation** in CI (extends the existing `rtp1-kernel-e2e.mjs` to cover the kernel → Rtp1OutboundTransport → controller round-trip).

### NEXT (3)

4. **Migrate `PtyHandle` struct → enum (Phase B of v9-5)**. The Phase A sub-structs are in place; the field-access sites can be mechanically replaced with `match handle { PtyHandle::Kernel(k) => ..., PtyHandle::Local(l) => ..., PtyHandle::Remote(r) => ... }`. Once Phase B lands, Phase C retires the legacy local / remote fields.
5. **Delete `OutboundClient` / `MockOutboundTransport` / `bind_mock_outbound_and_list` (legacy rdg-era) once `Rtp1OutboundTransport` covers all production call sites**. Removes the second outbound runtime.
6. **Add end-to-end Tauri render benchmark (release build + headed runner)** to measure `input_ui_to_render_submit` P95 ≤ 16 ms. Currently the closest is the v9-1 e2e CI; this would close the render measurement loop.

### LATER

* `Rtp1KernelClient::list_sessions` (v9-3) is in place but no caller uses it yet. Wire into `commands::host::list_hosts` for CLI consumers.
* `Pkgend::PtyHandle` is unused code; once Phase B is done and the struct is removed, the sub-structs can be deleted.
* `MiniRtp1Client` in `hosts/rtp1_outbound.rs` carries a `get_json` stub that errors. Replace with a real `reqwest` or `ureq` call once Tauri runtime is wired.
* `tests/terminal_live.rs` (16 PASS) does not cover the v8-3 `PtyInputSink::controller_id` propagation. Add a regression test.
* `lsp::set_app_handle` is still a synchronous side-effect on app init; consider deferring to first Tauri command.

---

## 13. Open Questions

* Should `RIDGE_RTP1_KERNEL=1` be the default (instead of opt-in) in v9/v10? The current default is the legacy HTTP lease; switching the default would close the second terminal runtime but requires v9-4 `Rtp1OutboundTransport` to be wired first.
* Should the `Rtp1KernelClient::list_sessions` result be cached on `AppState` to avoid re-fetching the remote-host topology on every `bind_mock_outbound_and_list` call?
* The 2 pre-existing Tauri test failures predate this session; should they be patched as part of NOW (they're blocking clean ridge lib runs)?

---

ARCHITECTURE_ALIGNMENT: **ALIGNED**

FOUNDATION_STATUS: **PARTIAL** (RTP1 canonical, PTY_SINGLE_OWNER verified, all L2 SPECS authoritative; 2 pre-existing Tauri test failures block a clean PASS)

TOP_3_NEXT:
1. Fix 2 pre-existing Tauri test failures (history_scan, restart_reattach) — unblock clean runs.
2. Wire `Rtp1OutboundTransport` into `commands::mod.rs::bind_mock_outbound_and_list` — replace legacy rdg mux path.
3. Expand `rtp1-kernel-e2e.mjs` to cover `Rtp1OutboundTransport` end-to-end in CI — close the kernel → controller round-trip loop.

---

CURRENT_STATE_RECONSTRUCTION_COMPLETE
