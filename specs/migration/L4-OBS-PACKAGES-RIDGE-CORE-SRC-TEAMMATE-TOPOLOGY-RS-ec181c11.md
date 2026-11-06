---
id: L4-OBS-PACKAGES-RIDGE-CORE-SRC-TEAMMATE-TOPOLOGY-RS-ec181c11
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CORE-SRC-TEAMMATE-2180eb7a
title: topology.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-core/src/teammate/topology.rs
test_targets: []
verified_by:
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e
---

# topology.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置 — 测试引用

旧测试已随 CHG-070 源链迁出。保留原始测试映射及 Git 归档证据，移除现役失效引用。其余原有测试目标保留，不按名称猜测替代覆盖。

原始 test_targets

```json
[
  "src/lib/remote/cloud/cloudHostTopologyLink.test.ts"
]
```

`src/lib/remote/cloud/cloudHostTopologyLink.test.ts` → `docs/archive/CHG-070/frontend-cloud-topology/cloudHostTopologyLink.test.ts`
