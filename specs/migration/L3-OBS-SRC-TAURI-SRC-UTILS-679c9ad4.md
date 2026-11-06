---
id: L3-OBS-SRC-TAURI-SRC-UTILS-679c9ad4
level: L3
parent: L2-OBS-SRC-TAURI-6056d0ef
title: src-tauri/src/utils module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/utils/cwd.rs
  - src-tauri/src/utils/logging.rs
  - src-tauri/src/utils/mod.rs
test_targets: []
---

# src-tauri/src/utils module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`src-tauri/src/utils/error.rs` → `docs/archive/CHG-070/desktop-retired-tree/utils/error.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 9708f489e2353011292fbc05bd194fbdd60bab26

`src-tauri/src/utils/pane_id.rs` → `docs/archive/CHG-070/desktop-retired-tree/utils/pane_id.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 8d3be1b60dc3cb83fb199315b1eeac2aa1f5c872

`src-tauri/src/utils/pty_log.rs` → `docs/archive/CHG-070/desktop-retired-tree/utils/pty_log.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 88ddd1580809ac76ee88a0bfe264fa951f749aed

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/utils/cwd.rs",
    "src-tauri/src/utils/error.rs",
    "src-tauri/src/utils/logging.rs",
    "src-tauri/src/utils/mod.rs",
    "src-tauri/src/utils/pane_id.rs",
    "src-tauri/src/utils/pty_log.rs"
  ],
  "testTargets": []
}
```
