# Wave 80: NLM residual audit

## Local requirements already landed

The follow-up NLM audit identified three local requirements, but current source and tests show they are already implemented:

- `REQ-RIDGE-MCP-AS-KERNEL-API-01`: `packages/ridge-mcp` has SQLite-backed `hub_messages`, Inbox/Task/Event routing, idempotency, delivery receipts, generation/lease fencing, and persistence rehydration. `cargo test -p ridge-mcp --lib --quiet`: 90 passed.
- `REQ-RIDGE-KERNEL-HOST-01`: the independent kernel host/domain PTY lifecycle is present. `cargo test -p ridge-kernel --lib --quiet`: 49 passed; `cargo test -p ridge-cli --test kernel_lifecycle_e2e --quiet`: 3 passed.
- `REQ-CODEX-RENDER-STABILITY-01`: worker `applyDelta` carries a per-pane `frameId`; `renderWorker` rejects invalid or non-increasing frames via `lastAppliedFrameId`. Focused worker tests: 53 passed.

These are implementation-complete locally; no speculative refactor was added.

## Remaining external gates

- `REQ-REMOTE-SMOOTH-STATE-02` / `REQ-MOBILE-REMOTE-STATE-01`: physical iOS/Android PWA suspend/resume and soft-keyboard anchor evidence.
- `REQ-20260730-01` quality portion: a fresh SonarQube server analysis with the configured CI token.
- `REQ-TERMINAL-RASTER-01`: DPR 1.25/1.5/2 native PowerShell comparison matrix on real WebView2/device conditions.
- `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01`: clean mobile profile/extension isolation A/B evidence; source inspection alone is insufficient.
- RDG public/TURN and third-party Runtime/A2A paths: only LAN/local CDP has current live evidence; no credential or physical-network evidence was invented.

No release, push, or publish was performed.
