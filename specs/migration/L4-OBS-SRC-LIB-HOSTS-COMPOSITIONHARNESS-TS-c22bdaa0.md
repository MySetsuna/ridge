---
id: L4-OBS-SRC-LIB-HOSTS-COMPOSITIONHARNESS-TS-c22bdaa0
level: L4
parent: L3-OBS-SRC-LIB-HOSTS-71cf76ee
title: compositionHarness.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/hosts/compositionHarness.ts
test_targets:
  - src/lib/hosts/compositionHarness.test.ts
  - src/lib/hosts/foreignHistorySession.test.ts
  - src/lib/hosts/hostConnectFlow.test.ts
  - src/lib/hosts/hostControlSurface.test.ts
  - src/lib/hosts/hostForest.test.ts
  - src/lib/hosts/hostSessionIsolation.test.ts
  - src/lib/hosts/remotePaneBindings.test.ts
public_interface:
  - "export function compositionAllGreen(): boolean"
  - "export function compositionReport(): string"
  - "export function runAllCompositionScenarios(): CompositionScenario[]"
  - "export function scenarioGitOrch(): CompositionScenario"
  - "export function scenarioHitlHostsUi(): CompositionScenario"
  - "export function scenarioIsolationPump(): CompositionScenario"
  - "export function scenarioOutboundHistory(): CompositionScenario"
  - "export function scenarioProtocolLink(): CompositionScenario"
  - export interface CompositionScenario
verified_by:
  - TEST-OBS-SRC-LIB-HOSTS-COMPOSITIONHARNESS-TEST-TS-911c519a
  - TEST-OBS-SRC-LIB-HOSTS-FOREIGNHISTORYSESSION-TEST-TS-0de097ce
  - TEST-OBS-SRC-LIB-HOSTS-HOSTCONNECTFLOW-TEST-TS-a765764d
  - TEST-OBS-SRC-LIB-HOSTS-HOSTCONTROLSURFACE-TEST-TS-3e384126
  - TEST-OBS-SRC-LIB-HOSTS-HOSTFOREST-TEST-TS-b3e38a29
  - TEST-OBS-SRC-LIB-HOSTS-HOSTSESSIONISOLATION-TEST-TS-a2500470
  - TEST-OBS-SRC-LIB-HOSTS-REMOTEPANEBINDINGS-TEST-TS-2377e089
---

# compositionHarness.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
