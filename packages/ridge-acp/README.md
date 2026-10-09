# `ridge-acp` — Ridge Product Layer M2

Unified Agent product layer above the frozen Ridge Runtime/Kernel/Remote stack.

- `AgentSession` — one per concept (Lead / Staff / solo). Single canonical event bus and snapshot.
- ACP adapter — stdio JSON-RPC 2.0. Configurable framing: `ContentLength` (LSP-style, used by `live_acp_agent` and codex-acp) or `NdJson` (used by opencode-acp). No external SDK dependency.
- `Commutation` coordinator — Lead + N Staff + 1 task. Owns relations and task ownership only. Does NOT transport ACP frames.
- `fake_acp_agent` — minimal deterministic fixture (M1).
- `live_acp_agent` — full protocol-surface fixture emitting plan → thinking → message → tool_call → approval → tool_call → turn_completed (M2).
- `broken_acp_agent` — fixture exercising malformed frames + duplicate responses (transport lifecycle tests).
- `AcpTransport` runs dedicated reader / writer / stderr-drain tokio tasks with bounded mpsc + broadcast (no lock-across-await, no orphan tasks on drop).
- Real ACP E2E verified against `opencode-acp` v2.0.6 (`opencode-ai` 1.18.31+) — see `tests/real_acp_opencode.rs`.
- Honest capability gap: opencode-acp returns `-32601` for `session/cancel` and `agent/render_artifact`; Ridge surfaces that unchanged (no fabrication).

Reuses `ridge_core::teammate::{AgentLifecycle, AgentRole, AgentTier}`. Does not touch frozen Runtime.

## Run the test suite

```bash
# From the package directory.
cargo test --lib
cargo test --test e2e_acp_lifecycle -- --nocapture
cargo test --test e2e_communion_coordination -- --nocapture
cargo test --test e2e_session_snapshot_json -- --nocapture
cargo test --test mobile_consumer_protocol -- --nocapture
cargo test --test transport_lifecycle -- --nocapture
cargo test --test real_acp_e2e -- --nocapture    # live_acp_agent fixture
cargo test --test real_acp_opencode -- --nocapture # requires `opencode` on PATH
```

The fixture binaries (`fake_acp_agent`, `live_acp_agent`, `broken_acp_agent`) are built automatically by `cargo test` via the integration-test `[[bin]]` rule; resolution uses `env!("CARGO_BIN_EXE_*")` (no `which`/`PATH` guessing).

## M2 Acceptance

See `docs/architecture/notes/PRODUCT-LAYER-M2-ACCEPTANCE.md` for the 9-category acceptance report (REAL_ACP_E2E / READER_LIFECYCLE / SESSION_STATE / APPROVAL_FLOW / DESKTOP_CHAT / MOBILE_CONSUMER / COMMUNION_LIVE_E2E / REMAINING_GAPS / NEXT_SLICE).

## Current status

- `packages/ridge-acp/` is a standalone crate (not yet a workspace member; awaits workspace-inclusion authorization — see G1 in the acceptance report).
- Desktop Chat Tauri bridge is `NEXT_SLICE` (NS-2) — gated on workspace inclusion.
