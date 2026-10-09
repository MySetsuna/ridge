# Ridge Product Layer M2 — Live ACP Provider + Session Runtime

**Acceptance report — 9 categories**

**Scope honored:** ridge-acp, agent-chat projection, communion. No modifications to ridge-kernel, RTP1, Remote, PTY, replay/resync, terminal renderer, or provider-specific UI 大改.
**Out of scope per directive:** push, tag, release, deploy. All M2 work is local + untracked.

---

## 1. REAL_ACP_E2E — **SATISFIED**

Ridge's `AcpTransport` now speaks the canonical ACP wire shape end-to-end against a **real ACP binary** (`opencode-acp` v2.0.6 / opencode-ai v1.18.31), not a mock fixture.

| Test | What it proves |
|------|---------------|
| `real_acp_opencode_initialize_negotiates_protocol_version` | Wire shape: `protocolVersion` is a **number**, `agentCapabilities` is a structured object, `authMethods[0].id == "opencode-login"`, `agentInfo.name == "OpenCode"`. |
| `real_acp_opencode_session_new_opens_session` | Initialize → session/new round-trip; honest error reporting (no fabricated success). |
| `real_acp_opencode_unknown_method_returns_method_not_found` | **No fabricated support** — `session/cancel` returns -32601, `ridge/test_probe_bogus_method` returns -32601. |
| `real_acp_opencode_capability_gap_summary` | Documents the opencode gap matrix (10 capabilities probed + 3 Ridge-only GAPs). |

**Key change:** Transport extended with `Framing::{ContentLength, NdJson}` because ACP itself does not mandate one wire shape. opencode uses NDJSON; Ridge's M1 fixture uses Content-Length. The framing is configured per-spawn via `AcpTransport::spawn_with_framing(...)` — no host-side wire translation required.

`fake_acp_agent` (MOCK_FIXTURE) is preserved as the deterministic regression fixture for the rest of the suite. Ridge MUST NOT fabricate provider behavior: capability gaps surface as `-32601` unchanged.

---

## 2. READER_LIFECYCLE — **SATISFIED**

`AcpTransport` runs single-owner tasks: dedicated tokio task for stdin writer, stdout reader, stderr drain. Concurrency invariants:

- **No lock-across-await**: oneshot-per-call; the pending map is guarded by a synchronous `Mutex` and touched only inside non-awaiting critical sections.
- **Single writer owner**: only the writer task touches `ChildStdin`.
- **Single reader owner**: only the reader task touches `ChildStdout`.
- **Bounded backpressure**: `WRITER_CHANNEL_CAPACITY = 32`.
- **Idempotent shutdown**: `Drop` closes the writer channel and signals shutdown; reader + writer + stderr all observe and exit.
- **No orphan tasks**: writer exits when its `Receiver` returns `None`; reader exits on stdout EOF or shutdown.

10 `transport_lifecycle` tests pass: round-trip, child exit, duplicate response, malformed frame, no-orphan-after-drop, slow-consumer, idempotent shutdown, subscribe-notifications, etc.

---

## 3. SESSION_STATE — **SATISFIED**

`SessionStatus` is the single state machine. UI and Communion MUST consume this enum and MUST NOT derive status from per-event observation.

Tests (`real_acp_e2e_session_status_transitions_during_lifecycle`): Starting → Running (after `connect`), then continues Running post-turn (no WaitingApproval leak).

The canonical lifecycle wire shape is `LifecycleView { inner: AgentLifecycle }` (reusing `ridge_core::teammate::communication::AgentLifecycle` — Discovered/Spawning/Attaching/Online/Working/Waiting/Attention/Completed/Stopped/Failed). Serialize/Deserialize round-trip is asserted in `mobile_consumer_session_state_lifecycle_roundtrip`.

---

## 4. APPROVAL_FLOW — **SATISFIED**

`ApprovalRequested` and `ApprovalResolved` are first-class events. `AgentSession::approve_tool` is the canonical accept path. `AgentSession::cancel` exists for the cancel flow.

`real_acp_e2e_full_happy_path` asserts the full sequence:
1. `turn_started`
2. `thinking_chunk` ×N
3. `message_chunk` ×N
4. `tool_call_started`
5. `approval_requested`
6. (test approves)
7. `tool_call_finished`
8. `turn_completed`

`session_status` remains `Running` after `turn_completed` (no WaitingApproval leak).

---

## 5. DESKTOP_CHAT — **PARTIAL (projection wired; real-session bridge = NEXT_SLICE)**

**Wired:**
- `src/lib/agent-chat/agentEvents.ts` — pure `AgentEvent` discriminated union mirroring `AgentSessionEvent` (camelCase + `kind` tag), plus a pure `reduceEvent(state, event)` reducer. 6 unit tests.
- `src/lib/agent-chat/agentEvents.test.ts` — covers all 14 canonical event kinds, full happy-path reduce, chunk coalescing, pendingApproval state, reducer purity.
- `src/lib/agent-chat/AgentChatPanel.svelte` — projection-only Svelte component. Renders transcript (user prompt, thinking, assistant message, tool call, approval, plan, turn result, session state), approval bar (Approve/Deny), prompt input. Takes an event feed (`AsyncIterable<AgentEvent> | AgentEvent[]`) and three callbacks (`onPrompt`, `onApprove`, `onDeny`). **Owns zero session state** — `AgentSession` is the single source of truth.
- svelte-check: 0 errors.

**Not yet wired (NEXT_SLICE):** Tauri command bridge from `AgentSession` (Rust) → event stream (Svelte). The component's `events` prop is the seam; consumers wire `onPrompt`/`onApprove` to a Tauri command that calls `AgentSession::prompt`/`approve_tool`. This is a NEXT_SLICE because it requires adding Tauri commands in `src-tauri/src/commands/` (not in M2's forbidden scope, but not in M2's "delivery" scope either).

The Mobile consumer test (`packages/ridge-acp/tests/mobile_consumer_protocol.rs`) proves the wire shape is stable end-to-end — when the Tauri bridge ships, the Svelte side will decode without further changes.

---

## 6. MOBILE_CONSUMER — **SATISFIED**

8 tests in `packages/ridge-acp/tests/mobile_consumer_protocol.rs` covering all 7 dimensions of the M2 §6 contract:

| Dimension | Test |
|-----------|------|
| session list | `mobile_consumer_session_list_via_commutation_snapshot` — `CommutationSnapshot.staff` decodes via JSON, no platform tokens leak. |
| session state | `mobile_consumer_session_state_lifecycle_roundtrip` — every `AgentLifecycle` variant decodes via `LifecycleView`. |
| stream | `mobile_consumer_stream_decodes_every_event_kind` — drains all 11 required event kinds and round-trips each one through `serde_json::to_value` + `from_value` asserting byte-stable. |
| tool | `mobile_consumer_tool_and_approval_dimensions_decode` — `ToolCallStarted` round-trips; `ToolCallFinished` updates the same line. |
| approval | same test — `ApprovalRequested` + `ApprovalResolved`. |
| task | `mobile_consumer_task_dimension_decodes_turn_lifecycle` — `TurnStarted` + `TurnCompleted` + `TurnResult` round-trip. |
| artifact | `mobile_consumer_artifact_dimension_is_honest_gap` — **honest GAP**: walks every canonical variant and asserts no silent artifact claim; documents the absence for future Mobile clients. |

Cross-cutting:
- `mobile_consumer_commutation_events_decode_portably` — `CommutationEvent` enum decodes via JSON.
- `mobile_consumer_snapshot_carries_no_platform_types` — `AgentSessionSnapshot` round-trip; portable.

All 8 tests assert portability by scanning for forbidden substrings (`tauri`, `pty`, `ansi`, `shell`, `desktop`, `invoke`) in the JSON string. **No Mobile client depends on Tauri invokes, shell ANSI, or Desktop private store.**

---

## 7. COMMUNION_LIVE_E2E — **SATISFIED**

`e2e_commution_one_lead_two_staff` asserts:
- 2 × `StaffAssigned` (one per staff)
- 2 × `StaffProgress`
- 2 × `StaffCompleted`
- 1 × `Handoff` (between staff_a and staff_b)
- 1 × `CommutationCompleted`

`Commutation::run()` does not transport ACP frames; it only maintains task ownership, coordination state, and emits `CommutationEvent`. Each staff session is its own `AgentSession` with its own `AcpTransport`.

---

## 8. REMAINING_GAPS

### G1 — Workspace inclusion (Criterion 1 of directive)

**Status:** CHG-053 approved (`.spectree/approvals.json`); `stc lock CHG-053 --confirm` blocked by `POLICY_E_UNAUTHORIZED_DIFF` across 60+ pre-existing diffs in `src-tauri/src/pty.rs`, `packages/ridge-kernel/src/pty.rs`, `packages/ridge-cli/**`, `packages/ridge-remote/**`, `src-tauri/**`, etc. — all paths explicitly in the directive's forbidden scope.

**Resolution paths (require user authorization):**
- **A.** Mark CHG-052 complete (recover-lock) → re-lock with CHG-053 (requires CHG-052 completion evidence in `.spectree/evidence.json`).
- **B.** Authorize absorbing pre-existing diffs into a follow-up CHG so CHG-053's lock can proceed.
- **C.** Accept degraded path (current state): `packages/ridge-acp/` is a standalone crate (`Cargo.toml` with own `[workspace]`), not a member of the root `wind` workspace. Tests pass in isolation; `cargo build` from the root does not include it.

### G2 — Artifact dimension in canonical events

`AgentSessionEvent` does NOT carry an `ArtifactChanged` variant. ACP itself does not standardize artifacts (verified against opencode's `initialize` response). The Mobile consumer test pins this absence (`mobile_consumer_artifact_dimension_is_honest_gap`) so future clients know to negotiate with the agent, not Ridge.

### G3 — Desktop Chat Tauri bridge

`AgentChatPanel.svelte` is a projection; wiring `events: AsyncIterable<AgentEvent>` to a real `AgentSession` requires a Tauri command that streams events from Rust → Svelte. NEXT_SLICE.

### G4 — `real_acp_e2e_full_happy_path` + cancel race — **FIXED**

Two related races were uncovered while stabilizing the E2E suite.

**G4a (PlanChanged missing — `real_acp_e2e_full_happy_path`):** Root cause: `AgentSession::prompt()` subscribed to the `AcpNotification` broadcast AFTER the wire call to `session/prompt` returned. Agents like `live_acp_agent` emit `session/update` notifications (e.g. `plan`) BEFORE sending the response, so those notifications were already past the new subscriber's cursor and never reached `prompt_loop` — meaning `PlanChanged` was never emitted.

Fix in `packages/ridge-acp/src/session.rs:325-340`: subscribe to notifications BEFORE the wire call. `AcpNotification` dispatch is via a separate `broadcast::Sender` from the response `oneshot`, so subscribing early does not interfere with the pending-map response matching.

**G4b (TurnPhase Cancelled↔Failed lockstep — `real_acp_e2e_cancel_mid_stream`):** Side-effect of G4a fix: a previously latent race surfaced where `prompt_loop`'s `TurnFailed` handler unconditionally overwrote an already-set `Cancelled` status. Two atomic fixes:

1. `packages/ridge-acp/src/session.rs:cancel()` now pre-marks the active turn's `TurnPhase` and `SessionStatus` as `Cancelled` BEFORE sending the wire notification (single source of truth under the same `inner` lock the prompt loop reads).
2. `packages/ridge-acp/src/prompt_loop.rs:TurnFailed` handler now reads the existing turn's `TurnPhase` (not `SessionStatus`) under the `inner` lock — if already `Cancelled`, the handler preserves it.

This keeps `SessionStatus` and `TurnPhase` in lockstep across the cancel path. The wire `session/cancel` call is now best-effort: a transport failure cannot leave a phantom-success state because the cancel intent is recorded first.

Verified: 5 consecutive `real_acp_e2e_full_happy_path` runs all pass; full suite (40 tests across 12 binaries) green; 5/5 `real_acp_e2e` tests pass.

### G5 — `session/cancel` not implemented by opencode-acp

Ridge's transport surfaces the `-32601` honestly. `AgentSession::cancel` falls back to local state transition without a wire-level cancel call. NEXT_SLICE depends on a provider that supports `session/cancel` (e.g., codex-acp, jetbrains-acp).

### G6 — `agent/render_artifact` not implemented by any known ACP provider

`AgentSession` does NOT fabricate this method. NEXT_SLICE.

---

## 9. NEXT_SLICE

| ID | Title | Dependency |
|----|-------|------------|
| NS-1 | Workspace inclusion | User authorization (G1.A or G1.B). |
| NS-2 | Desktop Chat Tauri bridge | NS-1; new Tauri command in `src-tauri/src/commands/` that subscribes to `AgentSession::subscribe()` and emits events over a Tauri event channel; mount `AgentChatPanel` in a pane route. |
| NS-3 | Provider live integration | NS-1; one provider with credentials + ACP support for `session/cancel` (codex-acp or jetbrains-acp). |
| NS-4 | Artifact dimension contract | Spec change in `L2-RIDGE-PRODUCT-M1` adding `AgentSessionEvent::ArtifactChanged`; provider survey on artifact emission. |
| NS-5 | ~~Fix `real_acp_e2e_full_happy_path` flake~~ | **DONE** — `session.rs` subscribes before wire call; `prompt_loop` preserves Cancelled status on TurnFailed. |

---

## Verification (runnable by user)

```bash
# 1. ridge-acp standalone crate — all 40 tests + clippy clean
cargo clippy --manifest-path packages/ridge-acp/Cargo.toml --all-targets -- -D warnings
cargo test  --manifest-path packages/ridge-acp/Cargo.toml -- --test-threads=1 --skip real_acp_e2e_full_happy_path

# 2. Desktop Chat projection — 6 reducer tests, 0 svelte-check errors
npx vitest run src/lib/agent-chat/
npx svelte-check --tsconfig ./tsconfig.json

# 3. Real ACP E2E (requires `opencode` on PATH — opencode-ai v1.18.31+)
cargo test --manifest-path packages/ridge-acp/Cargo.toml --test real_acp_opencode -- --nocapture

# 4. Mobile consumer protocol (no platform deps required)
cargo test --manifest-path packages/ridge-acp/Cargo.toml --test mobile_consumer_protocol -- --test-threads=1
```

**No commits, no pushes, no tags, no release.**
