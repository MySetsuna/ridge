# Iteration 71 Contract — Commune MCP submit semantics

- Date: 2026-07-29
- Status: approved / implementation
- Requirement: `REQ-AGENT-COMMUNE-MCP-SUBMIT-03`

## Delivery boundary

1. `ridge_send_to_teammate` defaults to writing text and dispatching Enter.
2. Callers may request a draft only with explicit `submit:false`.
3. `ridge_send_and_submit` and `ridge_delegate_task` always submit.
4. Every submit path strips trailing CR/LF and appends exactly one CR.
5. Receipts distinguish terminal acceptance from Agent acknowledgement.

## Non-goals

- No host Ridge launch, shutdown, restart, or process manipulation.
- No claim that PTY acceptance proves Agent execution.
- No second input queue or terminal protocol.

## Deterministic gates

- `ridge-mcp` tests cover default submit, explicit draft, forced-submit aliases,
  and CR-not-LF encoding.
- Desktop legacy delegate/send-keys paths call the shared CR normalizer.
- Focused Ridge teammate tests compile and pass.
- `git diff --check`.
