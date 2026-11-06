---
id: L4-OBS-SRC-TAURI-SRC-COMMANDS-GIT-RS-8be46cbc
level: L4
parent: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
title: git.rs
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-070/desktop-retired-tree/commands/git.rs
test_targets: []
verified_by:
  - TEST-OBS-SRC-LIB-COMPONENTS-GITGRAPH-TEST-TS-69aabbd2
  - TEST-OBS-SRC-LIB-STORES-GITGUARDSTATS-TEST-TS-680ef77e
  - TEST-OBS-SRC-LIB-STORES-PANEGITSTATUS-TEST-TS-73bc40e2
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITACTIONS-TEST-TS-e28d50f2
  - TEST-OBS-SRC-REMOTE-LIB-REMOTEGITPANEL-TEST-TS-40e79e76
  - TEST-OBS-SRC-SHARED-SIDEBAR-GITREQUESTLIFECYCLE-TEST-TS-6975b120
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# git.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`src-tauri/src/commands/git.rs` → `docs/archive/CHG-070/desktop-retired-tree/commands/git.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 3a31c2889f45436f8ab251a3f21f4f00cb588c16

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/commands/git.rs"
  ],
  "testTargets": [
    "src/lib/components/GitGraph.test.ts",
    "src/lib/stores/gitGuardStats.test.ts",
    "src/lib/stores/paneGitStatus.test.ts",
    "src/remote/lib/remoteGitActions.test.ts",
    "src/remote/lib/RemoteGitPanel.test.ts",
    "src/shared/sidebar/gitRequestLifecycle.test.ts"
  ]
}
```
