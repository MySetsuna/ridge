---
id: L4-OBS-PACKAGES-RIDGE-CLI-SRC-RPC-RS-60de3504
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CLI-SRC-353b0498
title: rpc.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-cli/src/rpc.rs
test_targets:
  - packages/remote/src/shared/transport/paneRpcScheduler.test.ts
  - packages/remote/src/shared/transport/rpcClient.test.ts
  - packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-PANERPCSCHEDULER-TEST-TS-ad772920
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-RPCCLIENT-TEST-TS-605f379d
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTERPCSCHEDULER-TEST-TS-04be1fed
---

# rpc.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
