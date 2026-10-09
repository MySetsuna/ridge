---
id: L3-RIDGE-COMMUNION-M53-ACCEPTANCE
level: L3
title: M5.3 acceptance evidence
status: LOCKED
lifecycle: ACTIVE
parent: L2-RIDGE-COMMUNION-M53
code_targets:
  - packages/ridge-acp/tests/m53_acceptance.rs
  - src/lib/agent-chat/productAcceptance.test.ts
  - scripts/m53-product-acceptance.mjs
  - docs/architecture/notes/M5.3-ACCEPTANCE.md
  - vitest.config.ts
  - packages/ridge-acp/src/bin/live_acp_agent.rs
test_targets:
  - packages/ridge-acp/tests/m53_acceptance.rs
  - src/lib/agent-chat/productAcceptance.test.ts
  - packages/ridge-acp/tests/e2e_acp_lifecycle.rs
  - packages/ridge-acp/tests/e2e_communion_coordination.rs
  - packages/ridge-acp/tests/e2e_session_snapshot_json.rs
  - packages/ridge-acp/tests/m21_lifecycle_acceptance.rs
  - packages/ridge-acp/tests/mobile_consumer_protocol.rs
  - packages/ridge-acp/tests/real_acp_e2e.rs
---

Execute scenarios A simple chat, B reviewed A→(B,C)→D with same-turn approval and structured LeadFinal, and C restart/resume without repeating completed tasks. Run headed Ridge Desktop when available and credentialed ACP providers when available; otherwise explicitly NOT_RUN. Require npm test, npm run lint, npm run typecheck, controlled Worker tests repeated 50 times with zero flakes, and SpecTree verify/complete. Report every final M5.3 verdict; never claim COMMUNION_PRODUCT_READY=YES with known semantic bugs or timing flakes.
