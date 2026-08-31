---
id: L4-OBS-PACKAGES-RIDGE-CLI-SRC-HOST-RS-435d6ebb
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CLI-SRC-353b0498
title: host.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-cli/src/host.rs
test_targets:
  - packages/remote/src/shared/cloud/cloudHostBridge.test.ts
  - packages/remote/src/shared/cloud/cloudHostPaneSource.test.ts
  - packages/remote/src/shared/terminal/hostRemountPolicy.test.ts
  - packages/remote/src/shared/terminal/linkOpenHost.test.ts
  - src/lib/actions/hostSessionDrag.test.ts
  - src/lib/hosts/hostConnectFlow.test.ts
  - src/lib/hosts/hostControlSurface.test.ts
  - src/lib/hosts/hostForest.test.ts
  - src/lib/hosts/hostSessionIsolation.test.ts
  - src/lib/remote/cloud/cloudHostStore.test.ts
  - src/lib/remote/cloud/cloudHostTopologyLink.test.ts
  - src/lib/stores/hostReconnect.test.ts
  - src/lib/stores/hostReconnectProduct.test.ts
  - src/lib/stores/hosts.connect.test.ts
  - src/lib/stores/hosts.refresh.test.ts
  - src/lib/stores/hostsMutation.test.ts
  - src/lib/stores/hostsOutbound.test.ts
  - src/lib/stores/hostsOutboundProduct.test.ts
  - src/lib/stores/hostsPublic.test.ts
  - src/lib/terminal/hostPorts.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDHOSTBRIDGE-TEST-TS-731be498
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDHOSTPANESOURCE-TEST-TS-5698aba6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-HOSTREMOUNTPOLICY-TEST-TS-117c2a90
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-LINKOPENHOST-TEST-TS-19b69752
  - TEST-OBS-SRC-LIB-ACTIONS-HOSTSESSIONDRAG-TEST-TS-fe959003
  - TEST-OBS-SRC-LIB-HOSTS-HOSTCONNECTFLOW-TEST-TS-a765764d
  - TEST-OBS-SRC-LIB-HOSTS-HOSTCONTROLSURFACE-TEST-TS-3e384126
  - TEST-OBS-SRC-LIB-HOSTS-HOSTFOREST-TEST-TS-b3e38a29
  - TEST-OBS-SRC-LIB-HOSTS-HOSTSESSIONISOLATION-TEST-TS-a2500470
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTSTORE-TEST-TS-a2df9830
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-CLOUDHOSTTOPOLOGYLINK-TEST-TS-7c92f09e
  - TEST-OBS-SRC-LIB-STORES-HOSTRECONNECT-TEST-TS-e82e61be
  - TEST-OBS-SRC-LIB-STORES-HOSTRECONNECTPRODUCT-TEST-TS-9598a61c
  - TEST-OBS-SRC-LIB-STORES-HOSTS-CONNECT-TEST-TS-3c065b9c
  - TEST-OBS-SRC-LIB-STORES-HOSTS-REFRESH-TEST-TS-57fb3707
  - TEST-OBS-SRC-LIB-STORES-HOSTSMUTATION-TEST-TS-ef74d67d
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUND-TEST-TS-afc6c610
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUNDPRODUCT-TEST-TS-c9cf71df
  - TEST-OBS-SRC-LIB-STORES-HOSTSPUBLIC-TEST-TS-b49d4adf
  - TEST-OBS-SRC-LIB-TERMINAL-HOSTPORTS-TEST-TS-bed9ff49
---

# host.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
