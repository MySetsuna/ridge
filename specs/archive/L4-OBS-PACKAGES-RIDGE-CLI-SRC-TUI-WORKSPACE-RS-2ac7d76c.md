---
id: L4-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-WORKSPACE-RS-2ac7d76c
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-985fdbf4
title: workspace.rs
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-090/source/packages/ridge-cli/src/tui/workspace.rs
test_targets: []
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-WORKSPACESCOPE-TEST-TS-184beaf5
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-BEHAVIOR-TEST-TS-69e303c4
  - TEST-OBS-SRC-LIB-REMOTE-CLOUD-SHAREDWORKSPACEPROJECTION-TEST-TS-0dde9c6f
  - TEST-OBS-SRC-LIB-TEAMMATE-WORKSPACEMEMORY-TEST-TS-ac0bbd3e
  - TEST-OBS-SRC-REMOTE-LIB-WORKSPACETREE-TEST-TS-9753d5fd
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# workspace.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`packages/ridge-cli/src/tui/workspace.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/tui/workspace.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 155dd48292448423a553bb8f9da114c6cc118e51

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "packages/ridge-cli/src/tui/workspace.rs"
  ],
  "testTargets": [
    "packages/remote/src/shared/cloud/workspaceScope.test.ts",
    "src/lib/remote/cloud/sharedWorkspaceProjection.behavior.test.ts",
    "src/lib/remote/cloud/sharedWorkspaceProjection.test.ts",
    "src/lib/teammate/workspaceMemory.test.ts",
    "src/remote/lib/WorkspaceTree.test.ts"
  ]
}
```
