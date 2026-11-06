---
id: L4-OBS-SRC-TAURI-SRC-HOSTS-OUTBOUND-RS-b2497a1b
level: L4
parent: L3-OBS-SRC-TAURI-SRC-HOSTS-728bec8b
title: outbound.rs
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-090/source/src-tauri/src/hosts/outbound.rs
test_targets: []
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDLIFECYCLE-TEST-TS-9e8e8887
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDRECONNECT-TEST-TS-1dd36e13
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUND-TEST-TS-afc6c610
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUNDPRODUCT-TEST-TS-c9cf71df
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# outbound.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`src-tauri/src/hosts/outbound.rs` → `docs/archive/CHG-090/source/src-tauri/src/hosts/outbound.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 5aed94526ad79ee3f1f41a5971c0b0827f5e5a0a

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/hosts/outbound.rs"
  ],
  "testTargets": [
    "packages/remote/src/shared/hosts/outboundLifecycle.test.ts",
    "packages/remote/src/shared/hosts/outboundReconnect.test.ts",
    "src/lib/stores/hostsOutbound.test.ts",
    "src/lib/stores/hostsOutboundProduct.test.ts"
  ]
}
```
