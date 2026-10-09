---
id: L3-RIDGE-COMMUNION-M53-RUNTIME
level: L3
title: Canonical workflow runtime hardening
status: LOCKED
lifecycle: ACTIVE
parent: L2-RIDGE-COMMUNION-M53
code_targets:
  - packages/ridge-acp/src/task.rs
  - packages/ridge-acp/src/worker.rs
  - packages/ridge-acp/src/workflow.rs
  - packages/ridge-acp/src/test_harness.rs
  - packages/ridge-acp/src/session.rs
  - packages/ridge-acp/src/communion.rs
  - packages/ridge-acp/src/bin/fake_acp_agent.rs
  - packages/ridge-acp/src/prompt_loop.rs
  - packages/ridge-acp/src/provider.rs
test_targets:
  - packages/ridge-acp/tests/m53_acceptance.rs
---

Keep one canonical PlanStore and AgentSession. Live approvals continue the same turn without re-prompting. Lost Running/WaitingApproval restore as Interrupted and require explicit fresh-session Resume. Replace timing-flaky worker tests with controlled events/clock and repeat 50 times. Outcomes distinguish success, unsuccessful terminal, recoverable, and active. Aggregate all canonical task results, including restored/interrupted/blocked tasks; never silently discard persistence errors.
