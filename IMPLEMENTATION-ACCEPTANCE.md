# IMPLEMENTATION ACCEPTANCE — Terminal / Protocol / Remote

Audit scope: this session's Terminal / Protocol / Remote / Performance work,
measured against the four APPROVED L2 specs (`specs/L2-TERM-001.md`,
`specs/L2-PROTO-001.md`, `specs/L2-REMOTE-001.md`, `specs/L2-PERF-001.md`).

Authority: APPROVED Spec + current code + actual test output.
The prior "completion report" is **not** used as evidence.

## 1. Git / Change Inventory

```
branch  : main
HEAD    : 58e1ede0 (chore(release): prepare v0.1.86) — unchanged this session
commits this session : 0 (all work lives in working tree, uncommitted)
working tree status  :
  M packages/ridge-kernel/src/lib.rs
  M packages/ridge-kernel/src/pty.rs
  ?? packages/ridge-kernel/src/kernel_backed_handle.rs
  ?? packages/ridge-kernel/tests/conformance_contract.rs
  ?? packages/ridge-kernel/tests/conformance_kernel_backed.rs
  ?? packages/ridge-kernel/tests/conformance_replay.rs
  ?? packages/ridge-kernel/tests/conformance_runtime.rs
  ?? specs/L2-TERM-001.md
  ?? specs/L2-PROTO-001.md
  ?? specs/L2-REMOTE-001.md
  ?? specs/L2-PERF-001.md

deleted files : 0
legacy code removed : 0
tests added       : 4 conformance files (16 tests total, all PASS)
benchmark added   : 1 perf smoke (#[ignore]) in conformance_contract.rs
```

By domain (TERM / PROTO / REMOTE / PERF / RDG / OTHER):

```
TERM   : tests/conformance_contract.rs (7), conformance_kernel_backed.rs (4)
PROTO  : specs/L2-PROTO-001.md only — wire code: NO new commits
REMOTE  : tests/conformance_replay.rs (3), conformance_runtime.rs (2)
PERF   : specs/L2-PERF-001.md + 1 #[ignore] perf smoke printing 4.83 MiB/s hub-only
RDG    : 0 — no rdg/headless code change this session
OTHER  : specs/L2-TERM-001.md + kernel_backed_handle.rs wrapper type
```

Note: this session's `git log --oneline` shows no new commits. Every change
above is in the working tree only.

## 2. Spec → Code Traceability

Status values: `IMPLEMENTED` / `PARTIAL` / `NOT_IMPLEMENTED` / `CONFLICT` /
`UNVERIFIED`.

### TERM-001

| Requirement | Implementation | Code evidence | Test evidence | Status |
|---|---|---|---|---|
| kernel authoritative PTY byte ordering | PtyOutputHub publishes per-PTY monotonic seq in kernel | `packages/ridge-kernel/src/pty.rs` (`OutputState.next_seq`, `publish`, `attach`) | conformance_contract `output_seq_is_strictly_monotonic` | IMPLEMENTED at kernel hub layer; UNVERIFIED whether shell default create_pane actually routes through this hub |
| kernel authoritative PTY lifecycle | kernel PtyRegistry: spawn/destroy/begin_destroy/finish_destroy | `packages/ridge-kernel/src/pty.rs` `impl PtyRegistry` | kernel unit tests `pty::tests::interactive_bridge_delivers_input_to_child` etc. | IMPLEMENTED at kernel; shell default path does **not** use it (uses portable_pty directly) |
| RawByteStream / TerminalDelta / TerminalSnapshot / RenderFrame separation | types defined in `ridge_term::pty` and `remote_v2` | `packages/ridge-term/src/term/delta.rs`, `remote_v2.rs`, `packages/ridge-kernel/src/pty.rs` | conformance tests do not exercise RenderFrame (frontend layer) | PARTIAL (types exist; runtime path uses PaneParser + DeltaFrame, not the spec's clean four-shape contract) |
| replay / snapshot recovery contract | PtyOutputLease + bounded replay + Lagged | `pty.rs` `PtyOutputHub::publish` + `attach::Lagged` + `resync` | conformance_contract `bounded_replay_cap_triggers_lagged_after_eviction`, `lease_resync_resets_cursor_to_oldest` | IMPLEMENTED at kernel hub layer |
| client presentation-local state | shell PtyHandle.parser + delta_mode + pty_scrollback | `src-tauri/src/engine/parser.rs` `PaneParser` + `state.rs` `Workspace.terminals` | not regression-tested this session | IMPLEMENTED (existed pre-session) |

### PROTO-001

| Requirement | Implementation | Code evidence | Test evidence | Status |
|---|---|---|---|---|
| RTP1 envelope (magic `RTP1`, `efv`, type, flags, payload_len) | type definitions absent in code | `grep -rn "RTP1" packages/ src-tauri/` → 0 hits | none | NOT_IMPLEMENTED |
| envelope_format_version | spec-level only | n/a | none | NOT_IMPLEMENTED |
| negotiated protocol version (`client_min/max`, `server_version`) | spec-level only | n/a | none | NOT_IMPLEMENTED |
| runtime_epoch | `KernelEndpoint.started_at_unix + token` (NOT UUID v7) | `packages/ridge-kernel/src/registry.rs` `KernelEndpoint`; `src-tauri/src/kernel_lifecycle.rs` | conformance_runtime `kernel_instance_guard_is_exclusive_within_one_process` | PARTIAL (lock works; epoch derivation does not match spec) |
| terminal_id | PtyRegistry uses Uuid for PTYs | `packages/ridge-kernel/src/pty.rs` `spawn_command_for` | kernel unit tests | IMPLEMENTED |
| controller_id | `KernelBackedHandle::ControllerId` (newly added, scoped identity, no monotonicity) | `packages/ridge-kernel/src/kernel_backed_handle.rs` | conformance_kernel_backed `controller_id_scopes_identity_but_does_not_carry_sequence` | IMPLEMENTED at type level; NOT yet wired into shell attach path |
| output_seq | `PtyOutputHub` monotonic seq | `pty.rs` | conformance_contract | IMPLEMENTED at kernel |
| input_seq | type-level: `pty_input_lanes: (ws, pane, src) → Mutex<…>` | `src-tauri/src/state.rs::pty_input_lanes` | not regression-tested this session | PARTIAL (existing field, no wire contract yet) |
| realtime framing (no continuation) | `PtyOutputHub::publish` treats each `publish` as standalone frame | `pty.rs` | conformance_contract `output_seq_is_strictly_monotonic` | IMPLEMENTED at kernel; wire contract: NOT_IMPLEMENTED |
| snapshot chunk | type definitions only | `specs/L2-PROTO-001.md` | none in code | NOT_IMPLEMENTED |
| replay chunk | type definitions only | `specs/L2-PROTO-001.md` | none in code | NOT_IMPLEMENTED |
| `continuation` bit (chunk boundary) | spec-level only | n/a | none | NOT_IMPLEMENTED |
| protocol errors (`unknown_envelope`, `runtime_epoch_stale`, etc.) | spec-level only | n/a | none | NOT_IMPLEMENTED |
| legacy adapter / fallback layer | spec-level only; existing `RemotePtyEvent` / `bounded-seq-v1` paths remain | `packages/ridge-kernel/src/domain.rs:1225` `"protocol": "bounded-seq-v1"`; `src-tauri/src/types.rs::RemotePtyEvent`; `packages/ridge-tmux/src/lib.rs` ridge-remote-ws | none | PARTIAL (legacy paths are reachable; no adapter wires them into RTP1) |

### REMOTE-001

| Requirement | Implementation | Code evidence | Test evidence | Status |
|---|---|---|---|---|
| attach | `HostRegistry::attach_host_session` + `attach_transaction: Mutex<()>` | `src-tauri/src/hosts/mod.rs` lines ~1040-1216 | conformance_runtime | PARTIAL (concurrent attach guarded; no wire-level `runtime_epoch` check) |
| detach | `detach_host_session` (Tauri command) + `OutboundClient.unsubscribe` | `hosts/mod.rs::detach_host_session` | none this session | IMPLEMENTED at code; runtime_epoch behavior UNVERIFIED |
| reconnect | `OutboundClient::connect_and_list` + `ReconnectSupervisor` | `hosts/outbound.rs` + `hosts/reconnect_supervisor.rs` | none this session | IMPLEMENTED at code (existing pre-session) |
| resume by since_output_seq | `kernel::pty::attach_output(after_seq)` + `KernelBackedHandle::attach(..., since)` (new) | `pty.rs::attach_output`; `kernel_backed_handle.rs::attach` | conformance_replay `attach_with_since_seq_resumes_after_given_cursor`; conformance_kernel_backed `attach_with_since_seq_advances_only_after_observed_frames` | IMPLEMENTED at kernel + new wrapper type; wire-level end-to-end UNVERIFIED |
| replay | `PtyOutputLease::next` + `KernelBackedHandle::next` | `pty.rs`; `kernel_backed_handle.rs::next` | conformance_replay `replay_does_not_yield_frames_before_since_seq` | IMPLEMENTED |
| desync (Lagged) | `PtyOutputRead::Lagged` | `pty.rs` `PtyOutputHub::publish` | conformance_contract `bounded_replay_cap_triggers_lagged_after_eviction` | IMPLEMENTED |
| snapshot resync | `PtyOutputLease::resync` + `KernelBackedHandle::resync` | `pty.rs`; `kernel_backed_handle.rs::resync` | conformance_contract `lease_resync_resets_cursor_to_oldest` | IMPLEMENTED at kernel; not yet integrated into remote reconnect |
| stale runtime_epoch rejection | NOT IMPLEMENTED on the wire path | `grep -rn "runtime_epoch" src-tauri/src/hosts/` → 0 hits | conformance_kernel_backed tests cursor advancement only | NOT_IMPLEMENTED in runtime path |
| rediscovery | NOT IMPLEMENTED on the wire path | n/a | none | NOT_IMPLEMENTED |
| Terminal Exited semantics (notify / reject input / replay allowed / attachment readable) | terminal exit fires `session_event{event:"exited"}` in spec; production: `kill_pane` → `outbound.rs::OutboundClient` cleanup | `hosts/outbound.rs` | none this session | UNVERIFIED |
| attachment remains readable after Exited | spec-level only | n/a | none | NOT_IMPLEMENTED |

### PERF-001

| Requirement | Implementation | Test evidence | Status |
|---|---|---|---|
| instrumentation | tracing + PerformanceObserver planned but PerformanceObserver not wired | conformance `perf_output_throughput_smoke` (1 test, release, hub-only) | PARTIAL |
| baseline | `perf_output_throughput_smoke` outputs `4.83 MiB/s` for hub-only single subscriber release | see test | PARTIAL (single dimension; no LAN/WAN, no input latency, no render_submit_cost) |
| before / after | none (no optimization attempted) | n/a | NOT_IMPLEMENTED |
| fault injection | none this session | none | NOT_IMPLEMENTED |
| soak | none | none | NOT_RUN |
| CPU / memory / latency / throughput | only `pty_output_throughput` (hub-only) measured | as above | PARTIAL |
| `physical_presentation_latency` | UNMEASURED | n/a | UNMEASURED (correct per spec §3.1) |

## 3. PTY Single Ownership Audit

```
PTY_SINGLE_OWNER: PARTIAL  (verified against the actual code, not assumed)
```

Evidence (from `grep`):

- shell `state.rs::PtyHandle`:
  - line 53: `pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>`
  - line 54: `pub writer: Arc<Mutex<Box<dyn Write + Send>>>`
  - line 267 (engine/pty.rs): `pub _child: Option<Box<dyn portable_pty::Child + Send + Sync>>`
  - line 271: `pub kernel_ref: Option<crate::engine::kernel_pty::KernelPtyRef>`
- shell `commands/terminal.rs`:
  - line 6: `use portable_pty::{native_pty_system, CommandBuilder, PtySize};`
  - line 786: `spawn_pty_reader(state.clone(), workspace_id, pane_id, reader);`
  - line 1166: `let pty_system = native_pty_system();`
  - line 1425: `let portable_pty::PtyPair { master, slave } = pair;`
  - line 1718: `spawn_pty_reader(state.clone(), workspace_id, pane_id, reader);`
- `kernel_pty.rs` exists and provides `make_master` / `make_writer` /
  `KernelPtyRef`. It is imported in `commands/terminal.rs` line 19. Used at
  `commands/terminal.rs:879` and `:934` (specific call sites). The
  `create_pane_inner` default path at line 405 / `:1166` still uses
  `portable_pty::native_pty_system()` directly.

Operations (per `create_pane_inner`):
- **spawn PTY**: shell, via `portable_pty` directly.
- **owns child**: shell PtyHandle `_child` field.
- **owns master**: shell PtyHandle `master` field.
- **owns writer**: shell PtyHandle `writer` field.
- **reads output**: shell, via `spawn_pty_reader` (engine/pty.rs).
- **assigns output_seq**: shell `pty_pane_registry` / `PaneDeltaMailbox`,
  **separate from** kernel `PtyOutputHub` seq.
- **handles input / resize / destroy**: shell `commands/terminal.rs::write_to_pty`
  / `resize_pane` / `kill_pane`.

The kernel PtyRegistry authoritative path exists but is only reached when
`kernel_ref` is non-None — i.e. for explicit kernel-driven PTYs (foreign
attach, kernel-spawned from CLI `rdg host`). It is **not** the default path
for the desktop app's `create_pane`.

Verdict: PTY_SINGLE_OWNER = **PARTIAL**. Dual-source is real and active on
the desktop default path. The kernel_pty module is an alternate, opt-in
path. Phase 2 deliverable (kernel-backed handle wrapper type + cursor
advancement conformance tests) is implemented at the type level but is
not yet wired into `create_pane_inner`.

## 4. Dead / Legacy Path Audit

```
ACTIVE_CANONICAL                 : portable_pty direct spawn via commands/terminal.rs::create_pane_inner (desktop default)
                                   kernel PtyRegistry (kernel-driven PTYs + rdg host + foreign attach)
                                   RemotePtyEvent (LAN / cloud remote)
                                   ridge-remote-ws mux (rdg controller <-> host)
                                   bounded-seq-v1 HTTP (kernel domain)
                                   PaneParser (desktop parser)
                                   pty_scrollback (desktop mirror)
ACTIVE_LEGACY_ADAPTER            : kernel_pty (used at commands/terminal.rs:879, :934 — alternate path, NOT default)
DEPRECATED_BUT_REACHABLE         : shell PtyHandle.kernel_ref (Option, currently None by default)
                                   shell PtyHandle.native_ref / _child / native_cancel (legacy fields, still populated on default path)
                                   PtyOutputLease + PtyOutputHub (kernel authoritative, reachable via kernel HTTP)
                                   legacy OutboundClient mock transport + RemotePtyEvent::RawBytes (LAN remote)
                                   bounded-seq-v1 protocol (kernel domain, deployed for any HTTP client that does not speak RTP1)
                                   RemotePtyEvent::RawBytes / SemanticDelta / Metadata / Resize (LAN WS frame shape)
                                   ridge-remote-ws mux with channel-prefix demux (rdg controller)
                                   OutboundTransport mock impl (tests)
DEPRECATED_BUT_REACHABLE         : ReconnectSupervisor phase name "Succeeded/Idle" uses "Idle" — distinct from
                                   terminal Idle (which was already removed per L2-REMOTE-001 §3.1 freeze); risk
                                   of terminology confusion in future readers.
DEAD                             : (none confirmed; SPEC-TERM-001 §2.5 obsoleted paths may exist but were
                                   not surfaced as fully dead in this audit)
UNKNOWN                          : bounded-seq-v1 may or may not still serve live HTTP clients in the field;
                                   no call-site survey was performed.
```

Two authoritative active paths confirmed:

1. shell direct PTY (portable_pty + PaneParser + pty_scrollback + DeltaFrame
   to Tauri Channel)
2. kernel PtyRegistry + PtyOutputHub + PtyOutputLease (reached via foreign
   attach + kernel-driven CLI PTYs + kernel HTTP domain)

Both are active. **Risk: byte ordering divergence is possible in theory**
(the two paths emit different seq spaces; the contract that they never
overlap for the same terminal is implicit, not enforced by shared code).
This is the dual-source risk that the spec calls out.

## 5. Protocol Reality Audit

```
RTP1_STATUS: TYPE_ONLY  (spec only; no production wire code this session)
```

Trace a real terminal:

| Channel | Protocol in use | Evidence |
|---|---|---|
| Desktop local PTY output | bytes from `portable_pty` master → `spawn_pty_reader` → `pty-output-{ws}-{pane}` Tauri event → JS `Terminal.feed(bytes)` (RawByteStream-ish) | `engine/pty.rs::spawn_pty_reader`, `src-tauri/src/lib.rs::handle_pty_output` |
| Desktop local PTY parser → render | `PaneParser` (engine/parser.rs) emits `DeltaFrame` (postcard) via `PaneDeltaMailbox` → Tauri Channel → JS `take_pane_delta_frame` (TerminalDelta) | `engine/parser.rs`, `state.rs::PaneDeltaMailbox` |
| Desktop → kernel | `kernel::client::running_endpoint` + various domain reads (git, agent roster, remote-hosts, workspaces) | `commands/*.rs` imports of `ridge_kernel::client::*` |
| LAN Remote (LAN host) | `RemotePtyEvent::{RawBytes, SemanticDelta, Metadata, Resize}` over WS, plus `cloud_pane_raw_subs` / `cloud_pane_terminal_subs` (Tauri events forwarded to WS) | `src-tauri/src/types.rs`, `src-tauri/src/commands/cloud_pane.rs`, `ridge-remote/src/server_app.rs` |
| Cloud Remote | WebRTC + E2EE DataChannel; `subscribe_pane_raw` / `subscribe_pane_terminal_v2` (TerminalSnapshot / TerminalDeltaFrame over Tauri event) | `src-tauri/src/commands/cloud_pane.rs`, `packages/ridge-term/src/remote_v2.rs` |
| rdg / headless | `ridge-remote-ws` mux + JSON-RPC 2.0 + channel-prefix demux | `packages/ridge-cli/src/mux.rs` + `rpc.rs` |
| Kernel HTTP domain | `bounded-seq-v1` (named in `domain.rs:1225`) | `packages/ridge-kernel/src/domain.rs` |

RTP1 wire code: none in production. None in tests. The only RTP1-aware
artifacts are the spec markdown and the conformance test stubs in
`conformance_replay.rs` / `conformance_kernel_backed.rs` (which exercise
the kernel hub and the new wrapper type, not RTP1 itself).

## 6. Remote Recovery Reality

```
REMOTE_RECOVERY: PARTIAL
```

What is verified (hub-only / type-level conformance):

- `attach_with_since_seq_resumes_after_given_cursor` (PASS)
- `replay_does_not_yield_frames_before_since_seq` (PASS)
- `attach_with_since_seq_advances_only_after_observed_frames` (PASS)
- `kernel_instance_guard_is_exclusive_within_one_process` (PASS)
- `runtime_epoch_derivation_is_stable_within_one_endpoint` (PASS)

What is **NOT** verified end-to-end this session:

- Wire-level `runtime_epoch` is not carried by `OutboundClient::attach`.
  `grep -rn "runtime_epoch" src-tauri/src/hosts/` → 0 hits. So an old
  `runtime_epoch` cannot be rejected by the wire path today.
- Rediscovery flow: not implemented. After `error{runtime_epoch_stale}`,
  client must call `host_list_sessions` again, but no client code does
  this.
- Terminal `Exited` event delivery: spec-level only; production runtime
  uses `kill_pane` which clears `OutboundClient` subscriptions. The
  `session_event{event:"exited"}` field appears in spec, not in the WS
  event enum.
- Attachment remains readable after Exited: no spec `attachment` state
  machine in production. `OutboundClient` unsubscribe drops the local
  foreign view on detach; the remote session itself (kernel) remains.

No silent rebind / fake recovery was introduced this session, so the
"must NOT silently rebind" rule has not been violated. But the guard
that would enforce it (wire-level `runtime_epoch` check) is not in
place. Risk: if a future change introduces a "re-use last attach after
disconnect" shortcut, it could break the contract silently.

## 7. rdg / Headless Host Reality

```
HEADLESS_HOST: PARTIAL  (binary compiles; functional integration NOT verified this session)
```

`packages/ridge-cli/src/main.rs`:

```
name    = "rdg"
about   = "Ridge headless remote host for Linux/VPS"
subcommands (run_command dispatch):
  Some(Command::Tui(args))    => run_tui(args).await
  Some(Command::Login(args))   => run_login(args).await
  Some(Command::Remote(args))  => daemon::run(args.shell, args.cwd, args.root).await
  Some(Command::Connect(args)) => run_connect(args).await
  Some(Command::Tmux(args))    => run_tmux(args).await
  Some(Command::Host(args))    => host::run(args.port).await
  Some(Command::Mcp(args))     => ridge_mcp_bridge::run(args.url, args.token).await
  Some(Command::Kernel(args))  => run_kernel_command(args.command)
  None                         => run_dashboard().await
```

User-facing command: **`rdg`** (binary name). Not `ridge`. Subcommands
`rdg host`, `rdg remote`, `rdg tmux`, `rdg kernel …` are all implemented
in source.

What was NOT exercised this session:

- Running `rdg` against a live kernel.
- Running `rdg host` and connecting a controller.
- Round-trip input / output / resize over `ridge-remote-ws`.
- Reconnect over `rdg` → kernel.

`cargo test -p ridge-cli --no-run` passed (binary compiles, lib unit
tests compile). But the binary itself was not started and no end-to-end
invocation was performed this session.

## 8. Build / Test Verification

| Suite | Result | Notes |
|---|---|---|
| `cargo check -p ridge-kernel` | PASS | 1m 18s cold; 6s warm |
| `cargo test -p ridge-kernel` | **70 PASS / 0 FAIL / 1 ignored** | 52 pre-existing lib unit tests + 2 new kernel_backed_handle + 7 conformance_contract + 4 conformance_kernel_backed + 3 conformance_replay + 2 conformance_runtime + 1 ignored perf smoke |
| `cargo check -p ridge-cli` | PASS | 1m 05s; 1 dead-code warning (`UserBrief.is_trial`) |
| `cargo check --workspace --exclude ridge-term` | PASS | 40.82s; 67 warnings (all pre-existing dead_code / unused_imports) |
| `cargo test -p ridge-cli` | **NOT RUN** | binary builds; no integration invocation this session |
| `pnpm test` (`vitest run`) | **ENV_BLOCKED** | `pnpm install` aborted: `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY`. No vitest suite ran. Frontend unit tests: UNVERIFIED this session. |
| `pnpm e2e:shell` (wdio) | **NOT RUN** | requires Tauri release build + tauri-driver install; not invoked |
| `pnpm e2e:perf` (wdio perf) | **NOT RUN** | requires release build; not invoked |
| `cargo test --release --include-ignored perf_output_throughput_smoke` | PASS | baseline = **4.83 MiB/s** hub-only single subscriber, single-thread, release |

Total Rust tests passing this session: **70 PASS / 0 FAIL** (kernel crate).
Other Rust crates: compile-checked only.
Frontend tests: ENV_BLOCKED.
E2E: NOT_RUN.

## 9. Terminal Compatibility Verification

```
UNVERIFIED
```

No compatibility scenarios were executed in this session. SPEC-TERM-001
§4 lists the capability matrix (UTF-8 / CJK / emoji / combining / cursor /
CSI / OSC / alt-screen / 256-color / true-color / mouse / bracketed
paste / resize / scrollback / clear / title / hyperlink). Status values
are sourced from the spec itself; this audit did not re-run those
tests, so no claim can be made that they currently pass or fail.

Specifically NOT exercised:
- bash / PowerShell
- vim, fzf, htop/lazygit
- CJK wide chars, emoji ZWJ
- resize storm
- ANSI colors, cursor, alt-screen
- bracketed paste, mouse

If regressions exist in these areas, this session's changes did not
introduce or fix them.

## 10. Performance Evidence

| Metric | Before | Current | Delta | Confidence |
|---|---|---|---|---|
| `pty_output_throughput` (kernel hub-only, release, single-thread, single subscriber) | not measured pre-session | **4.83 MiB/s** (256 KiB ceiling reached; Lagged fired at hub cap) | n/a | low (one sample, one configuration) |
| `input_ui_to_pty` | not measured | not measured | n/a | n/a |
| `pty_roundtrip` | not measured | not measured | n/a | n/a |
| `output_to_render_submit` | not measured | not measured | n/a | n/a |
| `input_ui_to_render_submit` | not measured | not measured | n/a | n/a |
| `lan_input_to_render_submit` | not measured | not measured | n/a | n/a |
| `render_submit_cost` | not measured | not measured | n/a | n/a |
| `physical_presentation_latency` | UNMEASURED | UNMEASURED | n/a | n/a (correct per L2-PERF-001 §3.1) |
| CPU idle / high-output | not measured | not measured | n/a | n/a |
| memory per terminal | not measured | not measured | n/a | n/a |
| `reconnect_time` / `replay_time` / `snapshot_resync_time` | not measured | not measured | n/a | n/a |
| `stale_epoch_detection_latency` / `rediscovery_latency` | n/a (feature not implemented) | not measured | n/a | n/a |

```
PERFORMANCE_REGRESSION: UNKNOWN
```

No baseline existed before this session to regress against; the single
measurement taken is hub-only and does not reflect end-user latency.

## 11. Stability / Soak Evidence

```
SOAK: NOT_RUN
```

| Metric | Value |
|---|---|
| duration | 0 |
| terminal count | 0 |
| workload | none |
| reconnect count | 0 |
| fault injection count | 0 |
| peak memory | not measured |
| memory growth | not measured |
| CPU drift | not measured |
| thread/task growth | not measured |
| leaked lease / subscription | not surveyed |
| crashes | none observed |
| deadlocks | none observed |
| protocol violations | not surveyed |

Soak harness not implemented. Unit tests are not a substitute for soak.

## 12. Architecture Result (post-this-session, grounded in current code)

```
┌────────────────── 桌面 shell (ridge.exe, Tauri v2) ──────────────────┐
│  WebView SPA                                                         │
│      │ Tauri invoke                                                   │
│      ▼                                                                │
│  commands/* (state.rs::AppState)                                     │
│      ├── create_pane_inner (DEFAULT, ACTIVE)                         │
│      │     └── commands/terminal.rs:1166 native_pty_system()          │
│      │         └── shell PtyHandle { master, writer, _child, parser, │
│      │                              delta_mode, kernel_ref=None }       │
│      │             ├── engine/pty.rs::spawn_pty_reader (ACTIVE)       │
│      │             └── engine/parser.rs::PaneParser → delta frames    │
│      │                                                                  │
│      ├── kernel_pty proxy (ALTERNATE, REACHABLE)                     │
│      │     └── commands/terminal.rs:879/934 (specific call sites)    │
│      │         └── engine/kernel_pty.rs (implements portable_pty traits │
│      │             against kernel PtyRegistry over HTTP)               │
│      │                                                                  │
│      ├── attach_host_session (FOREIGN ATTACH)                        │
│      │     └── hosts/mod.rs:1040-1216                                 │
│      │         └── OutboundClient (WS)                                │
│      │             └── foreign pane view = local PtyHandle.remote_ref  │
│      │                                                                  │
│      └── cloud pane subscribe                                          │
│            └── commands/cloud_pane.rs                                 │
│                └── Tauri events: RawBytes / SemanticDelta / Snapshot   │
│                                                                          │
│  RemotePtyEvent (LAN WS frame shape) — ACTIVE                          │
│  TerminalSnapshot / TerminalDeltaFrame (cloud pane) — ACTIVE            │
│  pty_scrollback (8 MiB block store) — ACTIVE                          │
│  PaneDeltaMailbox (≤8 KiB deltas / frame) — ACTIVE                     │
└────────────────────────────────────────────────────────────────────────┘
                     │  HTTP kernel JSON-RPC domain (x-ridge-kernel-token)
                     ▼
┌────────────── ridge-kernel (independent process, axum 0.7) ──────────┐
│  PtyRegistry (PTY process authority)                                  │
│  PtyOutputHub  (256 KiB / 256 frames bounded replay)                  │
│  PtyOutputLease (cursor + long-poll + Lagged + resync)                │
│  KernelBackedHandle (NEW wrapper type, not yet wired to shell default)│
│  WorkspaceGraph / TopologyGraph / RemoteHostTopology                  │
│  HttpServer: /v1/health /v1/status /v1/shutdown                        │
│            /v1/domain/{fs,git,agents,remote-hosts,workspaces,ptys,mcp}│
│  legacy: bounded-seq-v1 (named in domain.rs:1225)                      │
│  runtime_epoch: started_at_unix + token (NOT UUID v7)                 │
└────────────────────────────────────────────────────────────────────────┘
                     │
                     ▼
┌────────────── rdg (independent process, axum 0.7 + WebRTC) ──────────┐
│  ridge-remote-ws mux (JSON-RPC 2.0 + channel-prefix demux)            │
│  kernel_ctl::ensure_kernel_running → shares kernel with desktop       │
│  Commands: tui / login / remote / connect / tmux / host / mcp / kernel│
└────────────────────────────────────────────────────────────────────────┘

Authoritative ownership (where multiple sources exist):
  PTY process        : DUAL (kernel PtyRegistry + shell PtyHandle._child)
  PTY byte ordering  : DUAL (kernel PtyOutputHub seq + shell pty_pane_registry seq)
  Terminal lifecycle : DUAL (kernel + shell)
  Output rendering   : SHELL (delta frames via Tauri Channel)
  Terminal kernel domain state : KERNEL (workspace graph, agent roster, remote hosts)
  PTY input          : SHELL (write_to_pty → master; OR kernel_pty writer)
  Kernel CLI host    : KERNEL PtyRegistry (sole authoritative for rdg host)
```

The architecture is more dual-source than the prior implementation report
suggested. The kernel_pty module exists as an alternate path but is not
the default.

## 13. Remaining Technical Debt

### P0

1. **PTY single-owner not achieved on desktop default path.**
   `commands/terminal.rs::create_pane_inner` still calls
   `portable_pty::native_pty_system()` directly. Shell `PtyHandle` retains
   `master` / `writer` / `_child`. Kernel `PtyRegistry` is reachable only
   via explicit foreign attach / rdg host. PTY_SINGLE_OWNER = PARTIAL.

2. **RTP1 wire protocol not implemented.** Spec defines envelope /
   attachment / chunking / runtime_epoch contract, but no Rust source
   implements them. All wire code today is `RemotePtyEvent`,
   `bounded-seq-v1`, `ridge-remote-ws`. RTP1_CANONICAL = NO.

3. **Wire-level `runtime_epoch` rejection absent.**
   `OutboundClient::attach` does not carry `runtime_epoch`. A future
   change that adds silent reconnect reuse could violate the spec's
   "stale attach must be rejected" rule without any code-level alarm.
   grep `runtime_epoch` in `src-tauri/src/hosts/` → 0 hits.

4. **Frontend test environment cannot run.** `pnpm install` aborted in
   this session with `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY`. JS
   unit tests + wdio e2e suites: NOT_RUN this session.

### P1

5. **Dual-source risk on byte ordering.** Kernel `PtyOutputHub` and
   shell `pty_pane_registry` produce two independent seq spaces for the
   same PTY. No shared code enforces non-divergence.

6. **Terminal Exited semantics not wired.** Spec mandates
   `session_event{event:"exited", terminal_id}` delivery + attachment
   readable after Exited + replay/snapshot still allowed. Production
   runtime uses `kill_pane` + `OutboundClient` unsubscribe; no
   `session_event` is emitted in the WS frame shape today.

7. **`runtime_epoch` is not UUID v7.** Spec §3.5 (REMOTE-001) requires
   UUID v7 + decoupling from auth token. Code still derives from
   `started_at_unix + token`.

8. **`engine::kernel_pty` exists but is unreachable from default
   create_pane_inner.** Only specific call sites at
   `commands/terminal.rs:879` / `:934` use it. Either widen the default
   path or document it as foreign-attach-only.

9. **Performance: 1 dimension measured, 11 declared, 0 cross-process.**
   Baseline is hub-only. No LAN / WAN latency, no input latency, no
   render cost, no CPU/memory profile.

### P2

10. **Dead-code warnings** (67 in ridge lib + 1 in ridge-cli). Mostly
    historical. Documented but not cleared.

11. **ReconnectSupervisor "Idle" phase name** vs **terminal Idle**
    (REMOTE-001 §3.1 freeze removed terminal Idle). Terminology risk;
    future readers may conflate the two. Rename supervisor phase.

12. **chunked logical message conformance tests** at wire level not
    written; only `PtyOutputHub` levels tested.

13. **SpecTree graph nodes still 0.** CHG-001..007 are DRAFT; spec
    markdown files exist but are not in the SpecTree semantic graph.
    `stc apply --confirm` not executed.

## 14. Final Acceptance

```
TERM_ACCEPTANCE      : PARTIAL
PROTO_ACCEPTANCE     : PARTIAL
REMOTE_ACCEPTANCE    : PARTIAL
PERF_ACCEPTANCE      : PARTIAL
PTY_SINGLE_OWNER     : PARTIAL
RTP1_CANONICAL       : NO
HEADLESS_HOST        : PARTIAL
REMOTE_RECOVERY      : PARTIAL
PERFORMANCE_REGRESSION: UNKNOWN
SOAK                 : NOT_RUN

OVERALL_ACCEPTANCE   : CONDITIONAL_PASS

P0_BLOCKERS          : 4
  P0-1: PTY single-owner not achieved on desktop default path
  P0-2: RTP1 wire protocol not implemented
  P0-3: Wire-level runtime_epoch rejection absent
  P0-4: Frontend test environment cannot run in this session
```

This session produced:
- 4 APPROVED spec markdown files (chat-level owner override, not in
  SpecTree graph).
- 4 conformance test files (16 tests) all passing.
- 1 `KernelBackedHandle` wrapper type at the kernel crate.
- 1 perf smoke baseline (hub-only 4.83 MiB/s).

It did NOT produce:
- Any new git commits (all work is uncommitted working tree).
- Any wire-level RTP1 implementation.
- Any change to shell `create_pane_inner` default path.
- Any end-to-end verification of rdg headless host against a live kernel.
- Any frontend unit test or e2e test execution.
- Any soak or fault-injection harness.

The work is genuine and verifiable. It is not complete against the four
APPROVED specs as production-runtime changes. It is a defensible phase
deliverable for spec freezing + kernel-crate conformance scaffolding +
type-level PTY single-owner groundwork, but not a foundation completion.

---

**Caveat for next session:** this audit did not re-run any prior session's
PASS claims. It re-grepped code, re-ran cargo tests in this session,
and surfaced what is actually wired vs what is documented.
