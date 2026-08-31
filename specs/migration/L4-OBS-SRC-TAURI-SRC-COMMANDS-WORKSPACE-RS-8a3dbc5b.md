---
id: L4-OBS-SRC-TAURI-SRC-COMMANDS-WORKSPACE-RS-8a3dbc5b
level: L4
parent: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
title: workspace.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/commands/workspace.rs
test_targets:
  - packages/remote/src/shared/cloud/workspaceScope.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts
  - src/lib/remote/cloud/sharedWorkspaceProjection.test.ts
  - src/lib/teammate/workspaceMemory.test.ts
  - src/remote/lib/WorkspaceTree.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-WORKSPACESCOPE-TEST-TS-184beaf5
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-BEHAVIOR-TEST-TS-69e303c4
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TEST-TS-0dde9c6f
  - TEST-OBS-SRC-LIB-TEAMMATE-WORKSPACEMEMORY-TEST-TS-ac0bbd3e
  - TEST-OBS-SRC-REMOTE-LIB-WORKSPACETREE-TEST-TS-9753d5fd
---

# workspace.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
