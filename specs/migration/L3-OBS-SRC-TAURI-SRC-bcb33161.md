---
id: L3-OBS-SRC-TAURI-SRC-bcb33161
level: L3
parent: L2-OBS-SRC-TAURI-6056d0ef
title: src-tauri/src module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/deep_root.rs
  - src-tauri/src/kernel_lifecycle.rs
  - src-tauri/src/lib.rs
  - src-tauri/src/main.rs
  - src-tauri/src/reconnect_policy.rs
  - src-tauri/src/state.rs
  - src-tauri/src/taskbar.rs
  - src-tauri/src/tray.rs
test_targets: []
---

# src-tauri/src module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`src-tauri/src/remote_bridge.rs` → `docs/archive/CHG-090/source/src-tauri/src/remote_bridge.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob e056410a768ccc8cd0313171fdf1da3697a4c60e

`src-tauri/src/remote_host_impl.rs` → `docs/archive/CHG-090/source/src-tauri/src/remote_host_impl.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 4494aeb7dec320fa924d5ddcb45ff6e29705efb1

`src-tauri/src/types.rs` → `docs/archive/CHG-070/desktop-retired-tree/types.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 76eae6a7f0de58929ae5a488485acc85c8c6c2b8

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/deep_root.rs",
    "src-tauri/src/kernel_lifecycle.rs",
    "src-tauri/src/lib.rs",
    "src-tauri/src/main.rs",
    "src-tauri/src/reconnect_policy.rs",
    "src-tauri/src/remote_bridge.rs",
    "src-tauri/src/remote_host_impl.rs",
    "src-tauri/src/state.rs",
    "src-tauri/src/taskbar.rs",
    "src-tauri/src/tray.rs",
    "src-tauri/src/types.rs"
  ],
  "testTargets": []
}
```
