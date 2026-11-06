---
id: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
level: L3
parent: L2-OBS-SRC-TAURI-6056d0ef
title: src-tauri/src/commands module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/commands/clipboard_files.rs
  - src-tauri/src/commands/clipboard_image.rs
  - src-tauri/src/commands/mod.rs
  - src-tauri/src/commands/project.rs
  - src-tauri/src/commands/ridge_file.rs
  - src-tauri/src/commands/theme.rs
test_targets: []
---

# src-tauri/src/commands module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`src-tauri/src/commands/cloud_http.rs` → `docs/archive/CHG-070/desktop-retired-tree/commands/cloud_http.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 30ce95dbef08dcc0ad1c266718866e884d7b24a9

`src-tauri/src/commands/cloud_pane.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/cloud_pane.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob b42df5590aa4eb8a55ccf6328b05afd556a96ca5

`src-tauri/src/commands/fs_watch.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/fs_watch.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 05bcf133ccd1a69d2b53bb6b929f836469909e9e

`src-tauri/src/commands/git.rs` → `docs/archive/CHG-070/desktop-retired-tree/commands/git.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 3a31c2889f45436f8ab251a3f21f4f00cb588c16

`src-tauri/src/commands/pane.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/pane.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 314343138789235b4d229360e6cc922d96b9c853

`src-tauri/src/commands/process.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/process.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 88179974352aa243b3d8c3395edf80d9849c8b48

`src-tauri/src/commands/remote.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/remote.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 523971b401e85e70bc76866e31740e21e6d07b3e

`src-tauri/src/commands/settings.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/settings.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 5ecd5352926bdbf1da3e1336519dae6e3024a442

`src-tauri/src/commands/teammate.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/teammate.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 9d11e0a2830d58ac86dc2dc98c12b6eaa0471b98

`src-tauri/src/commands/terminal_font.rs` → `docs/archive/CHG-070/desktop-retired-tree/commands/terminal_font.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 1388ab7cc3282f2ecfac2e4dbafd5b02b4e5aa2e

`src-tauri/src/commands/terminal.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/terminal.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 15029d6855a7c6d1aba44e8a33a5d3d82961a7bd

`src-tauri/src/commands/watch.rs` → `docs/archive/CHG-070/desktop-retired-tree/commands/watch.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 0d505201fba8399eb95a2d68958dd637e5e5af1f

`src-tauri/src/commands/workspace.rs` → `docs/archive/CHG-090/source/src-tauri/src/commands/workspace.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 8a5204390d3311867474b6654f8ecc54317fe88e

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/commands/clipboard_files.rs",
    "src-tauri/src/commands/clipboard_image.rs",
    "src-tauri/src/commands/cloud_http.rs",
    "src-tauri/src/commands/cloud_pane.rs",
    "src-tauri/src/commands/fs_watch.rs",
    "src-tauri/src/commands/git.rs",
    "src-tauri/src/commands/mod.rs",
    "src-tauri/src/commands/pane.rs",
    "src-tauri/src/commands/process.rs",
    "src-tauri/src/commands/project.rs",
    "src-tauri/src/commands/remote.rs",
    "src-tauri/src/commands/ridge_file.rs",
    "src-tauri/src/commands/settings.rs",
    "src-tauri/src/commands/teammate.rs",
    "src-tauri/src/commands/terminal_font.rs",
    "src-tauri/src/commands/terminal.rs",
    "src-tauri/src/commands/theme.rs",
    "src-tauri/src/commands/watch.rs",
    "src-tauri/src/commands/workspace.rs"
  ],
  "testTargets": []
}
```
