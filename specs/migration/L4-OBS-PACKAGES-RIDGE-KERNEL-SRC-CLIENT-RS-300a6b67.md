---
id: L4-OBS-PACKAGES-RIDGE-KERNEL-SRC-CLIENT-RS-300a6b67
level: L4
parent: L3-OBS-PACKAGES-RIDGE-KERNEL-SRC-fc45dbfb
title: client.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-kernel/src/client.rs
test_targets:
  - packages/remote/src/shared/cloud/apiClient.refresh.test.ts
  - packages/remote/src/shared/cloud/apiClient.test.ts
  - packages/remote/src/shared/transport/rpcClient.test.ts
  - src/lib/lsp/lspClient.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-REFRESH-TEST-TS-ae2beee9
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-APICLIENT-TEST-TS-54ddea70
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-RPCCLIENT-TEST-TS-605f379d
  - TEST-OBS-SRC-LIB-LSP-LSPCLIENT-TEST-TS-bab87361
---

# client.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
