---
id: L4-OBS-SRC-LIB-REMOTE-CLOUD-CHECKINGATECARD-SVELTE-7b71a8aa
level: L4
parent: L3-OBS-SRC-LIB-REMOTE-CLOUD-643692b0
title: CheckinGateCard.svelte
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/remote/cloud/CheckinGateCard.svelte
test_targets:
  - src/lib/remote/cloud/cloudControllerBoot.integration.test.ts
  - src/lib/remote/cloud/cloudControllerBoot.test.ts
  - src/lib/remote/cloud/cloudHostStore.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.test.ts
verified_by:
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-INTEGRATION-TEST-TS-d930e02e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDCONTROLLERBOOT-TEST-TS-7f1c661a
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TEST-TS-a2df9830
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-BEHAVIOR-TEST-TS-69e303c4
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TEST-TS-0dde9c6f
---

# CheckinGateCard.svelte

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置 — 测试引用

旧测试已随 CHG-070 源链迁出。保留原始测试映射及 Git 归档证据，移除现役失效引用。其余原有测试目标保留，不按名称猜测替代覆盖。

原始 test_targets

```json
[
  "src/lib/remote/cloud/cloudControllerBoot.integration.test.ts",
  "src/lib/remote/cloud/cloudControllerBoot.test.ts",
  "src/lib/remote/cloud/cloudHostStore.test.ts",
  "src/lib/remote/cloud/cloudHostTopologyLink.test.ts",
  "src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts",
  "src/lib/remote/cloud/sharedWorkspaceProjection.test.ts"
]
```

`src/lib/remote/cloud/cloudHostTopologyLink.test.ts` → `docs/archive/CHG-070/frontend-cloud-topology/cloudHostTopologyLink.test.ts`
