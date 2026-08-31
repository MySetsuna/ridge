---
id: L4-OBS-SRC-TAURI-SRC-COMMANDS-GIT-RS-8be46cbc
level: L4
parent: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
title: git.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/commands/git.rs
test_targets:
  - src/lib/components/GitGraph.test.ts
  - src/lib/stores/gitGuardStats.test.ts
  - src/lib/stores/paneGitStatus.test.ts
  - src/remote/lib/remoteGitActions.test.ts
  - src/remote/lib/RemoteGitPanel.test.ts
  - src/shared/sidebar/gitRequestLifecycle.test.ts
verified_by:
  - TEST-OBS-SRC-LIB-COMPONENTS-GITGRAPH-TEST-TS-69aabbd2
  - TEST-OBS-SRC-LIB-STORES-GITGUARDSTATS-TEST-TS-680ef77e
  - TEST-OBS-SRC-LIB-STORES-PANEGITSTATUS-TEST-TS-73bc40e2
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITACTIONS-TEST-TS-e28d50f2
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITPANEL-TEST-TS-40e79e76
  - TEST-OBS-SRC-SHARED-SIDEBAR-GITREQUESTLIFECYCLE-TEST-TS-6975b120
---

# git.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
