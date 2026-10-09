---
id: CHG-053
title: Ridge Product Layer M1 — ACP Session + Agent Communion MVP
type: SEMANTIC_CHANGE
status: COMPLETED
affects:
  - L2-RIDGE-PRODUCT-M1
---

# Ridge Product Layer M1 — ACP Session + Agent Communion MVP

## §1 Semantic delta

Introduce a new crate `packages/ridge-acp/` as the unified Agent product layer:

- `AgentSession` — one per concept (Lead, Staff, solo). Holds event bus (broadcast), command sink (mpsc), lifecycle view, turn records.
- `AgentSessionEvent` — tagged-enum `Serialize` stream (camelCase + `kind`); covers SessionCreated/StateChanged/MessageStarted/Delta/Completed/ToolStarted/Output/Completed/ApprovalRequested/Resolved/PlanChanged/ArtifactChanged/SessionCompleted/SessionFailed per Goal §2.
- `AgentSessionSnapshot` — single render shape consumable by future Desktop/Mobile without Tauri/shell-ANSI coupling.
- ACP adapter — stdio JSON-RPC 2.0 + Content-Length framing. Methods: `initialize`, `session/new`, `session/prompt`, `session/cancel`, `session/approve` (M1 extension — spec has no approval method; explicitly labeled). Notifications: `session/update` (tag=kind) + `agent/thinking_chunk`, `agent/message_chunk`, `agent/tool_call`, `agent/tool_result`, `agent/request_approval`.
- `Commutation` coordinator — 1 Lead + N Staff + 1 task. Listens to Lead's `StaffAssignment` events, forwards prompts to named Staff, observes Staff completion, emits `LeadFinal`. Does NOT transport ACP frames, does NOT spawn processes (each AgentSession owns its transport).
- `MOCK_FIXTURE` — `fake_acp_agent` binary; deterministic stdio ACP server used by E2E harness.

Reuse (no fork):
- `ridge_core::teammate::communication::AgentLifecycle` — Discovered→Failed states.
- `ridge_core::teammate::model::{AgentRole, AgentTier}` — Leader/Worker/Observer + Base/Skilled/Expert.

## §2 Files

New:
- `Cargo.toml` — add `packages/ridge-acp` to workspace members.
- `packages/ridge-acp/Cargo.toml` — new crate manifest.
- `packages/ridge-acp/src/{lib,error,protocol,provider,lifecycle,transport,spawn,session,communion}.rs` — ≤600 lines each.
- `packages/ridge-acp/tests/common/mod.rs` — test harness helpers.
- `packages/ridge-acp/tests/e2e_acp_lifecycle.rs` — E2E-A.
- `packages/ridge-acp/tests/e2e_communion_coordination.rs` — E2E-B.
- `packages/ridge-acp/tests/e2e_session_snapshot_json.rs` — E2E-C.
- `packages/ridge-acp/tests/fixtures/fake_acp_agent/{Cargo.toml,src/main.rs}` — MOCK_FIXTURE binary.
- `specs/l2-ridge-product-m1-acp-session-communion.md` — new L2 spec.
- `.spectree/proposals/CHG-053.json` — proposal.

Not modified (frozen):
- `packages/ridge-kernel/**` (PTY, RTP1, transport, replay, backpressure, memory lifecycle).
- `packages/ridge-cli/**`, `packages/ridge-mcp/**`, `packages/ridge-remote/**`, `packages/ridge-core/**` (except reading teammate::{AgentLifecycle,AgentRole,AgentTier}).
- `src-tauri/**` and `src/**` (no Desktop UI change in M1).

## §3 Acceptance tests

- `cargo build -p ridge-acp` — green.
- `cargo test -p ridge-acp --lib` — green (lifecycle/protocol/snapshot unit tests).
- `cargo test -p ridge-acp --test e2e_acp_lifecycle -- --nocapture` — E2E-A PASS (asserts Initialized, SessionOpened, Plan, ThinkingChunk, MessageChunk, ToolCallStarted, ApprovalRequested, ApprovalResolved, ToolCallFinished, TurnCompleted, LifecycleChanged all observed).
- `cargo test -p ridge-acp --test e2e_communion_coordination -- --nocapture` — E2E-B PASS (asserts TaskAccepted, 2× StaffSpawned, StaffAssignment, 2× StaffFinal, LeadFinal).
- `cargo test -p ridge-acp --test e2e_session_snapshot_json -- --nocapture` — E2E-C PASS (asserts AgentSessionSnapshot JSON includes sessionId/lifecycle/provider/turns/phase/turnCompleted kind).
- `cargo clippy -p ridge-acp --all-targets -- -D warnings` — green.
- `git diff --stat` confirms `packages/ridge-kernel/**`, `packages/ridge-cli/**`, `packages/ridge-mcp/**`, `packages/ridge-remote/**`, `src-tauri/**` unchanged.
- `stc verify` — green.
- `stc complete` — round closed.

## §4 Status

DRAFT — pending `stc apply --confirm`.
