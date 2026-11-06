---
id: L4-OBS-SRC-TAURI-SRC-FS-TREE-RS-0009918d
level: L4
parent: L3-OBS-SRC-TAURI-SRC-FS-85347803
title: tree.rs
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-070/desktop-retired-tree/fs/tree.rs
test_targets: []
verified_by:
  - TEST-OBS-SRC-LIB-STORES-PANETREE-COVERAGE-TEST-TS-747ca055
  - TEST-OBS-SRC-LIB-STORES-PANETREE-TEST-TS-8fb828ce
  - TEST-OBS-SRC-REMOTE-LIB-TREESTATE-TEST-TS-bcbf35f8
  - TEST-OBS-SRC-REMOTE-LIB-WORKSPACETREE-TEST-TS-9753d5fd
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# tree.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`src-tauri/src/fs/tree.rs` → `docs/archive/CHG-070/desktop-retired-tree/fs/tree.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob ebd1821d5d0948c3dc6b79e854497bcc53306fa7

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/fs/tree.rs"
  ],
  "testTargets": [
    "src/lib/stores/paneTree.coverage.test.ts",
    "src/lib/stores/paneTree.test.ts",
    "src/remote/lib/treeState.test.ts",
    "src/remote/lib/WorkspaceTree.test.ts"
  ]
}
```
