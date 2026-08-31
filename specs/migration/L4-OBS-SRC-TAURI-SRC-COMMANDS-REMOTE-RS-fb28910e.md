---
id: L4-OBS-SRC-TAURI-SRC-COMMANDS-REMOTE-RS-fb28910e
level: L4
parent: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
title: remote.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/commands/remote.rs
test_targets:
  - packages/remote/src/shared/cloud/remoteAllowlist.test.ts
  - packages/remote/src/shared/teammate/hitlAuditRemote.test.ts
  - packages/remote/src/shared/transport/remoteInvokeAdmit.test.ts
  - packages/remote/src/shared/transport/remotePerfTrace.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - packages/remote/src/shared/transport/wsRemotePending.test.ts
  - packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts
  - packages/remote/src/shared/transport/wsRemoteUrl.test.ts
  - scripts/lib/remoteArtifactBundle.test.mjs
  - scripts/rdg-remote-e2e.test.ts
  - scripts/remote-createws-test.mjs
  - scripts/remote-runtime-last-error-attribution.test.ts
  - scripts/validate-remote-smoke-evidence.test.mjs
  - src/lib/hosts/remotePaneBindings.test.ts
  - src/lib/remote/remoteBootMode.test.ts
  - src/lib/stores/remoteStatus.test.ts
  - src/remote/lib/cloudRemote.test.ts
  - src/remote/lib/remoteGitActions.test.ts
  - src/remote/lib/RemoteGitPanel.test.ts
  - src/remote/lib/remoteQueries.test.ts
  - src/remote/lib/RemoteSidebar.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-REMOTEALLOWLIST-TEST-TS-ad14903f
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TEAMMATE-HITLAUDITREMOTE-TEST-TS-27ed6157
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-REMOTEINVOKEADMIT-TEST-TS-d1037371
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-REMOTEPERFTRACE-TEST-TS-a6430a11
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTE-BEHAVIOR-TEST-TS-8b7a590c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTEPENDING-TEST-TS-ee9c7d29
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTERPCSCHEDULER-TEST-TS-04be1fed
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTEURL-TEST-TS-a219c8e6
  - TEST-OBS-SCRIPTS-LIB-REMOTEARTIFACTBUNDLE-TEST-MJS-75c40031
  - TEST-OBS-SCRIPTS-RDG-REMOTE-E2E-TEST-TS-2c9bc9e5
  - TEST-OBS-SCRIPTS-REMOTE-CREATEWS-TEST-MJS-e9b94290
  - TEST-OBS-SCRIPTS-REMOTE-RUNTIME-LAST-ERROR-ATTRIBUTION-TEST-TS-812bdc3e
  - TEST-OBS-SCRIPTS-VALIDATE-REMOTE-SMOKE-EVIDENCE-TEST-MJS-a68aae34
  - TEST-OBS-SRC-LIB-HOSTS-REMOTEPANEBINDINGS-TEST-TS-2377e089
  - TEST-OBS-SRC-LIB-REMOTE-REMOTEBOOTMODE-TEST-TS-0c3742d1
  - TEST-OBS-SRC-LIB-STORES-REMOTESTATUS-TEST-TS-a66e6e00
  - TEST-OBS-SRC-REMOTE-LIB-CLOUDREMOTE-TEST-TS-8f8cb6d4
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITACTIONS-TEST-TS-e28d50f2
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITPANEL-TEST-TS-40e79e76
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEQUERIES-TEST-TS-42897c52
  - TEST-OBS-SRC-REMOTE-LIB-REMOTESIDEBAR-TEST-TS-743bbf5b
---

# remote.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
