---
id: L4-OBS-SRC-TAURI-SRC-ENGINE-PTY-RS-104cc347
level: L4
parent: L3-OBS-SRC-TAURI-SRC-ENGINE-7710ea45
title: pty.rs
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-070/desktop-retired-tree/engine/pty.rs
test_targets: []
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PTYBRIDGE-TEST-TS-3f30eb8e
  - TEST-OBS-SCRIPTS-CDP-PTY-STATE-TEST-MJS-462b9d95
  - TEST-OBS-SRC-LIB-TERMINAL-PTYRUNTIMESNAPSHOT-TEST-TS-a223e946
  - TEST-OBS-SRC-LIB-TERMINAL-PTYWRITEQUEUE-TEST-TS-b49b9e91
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# pty.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`src-tauri/src/engine/pty.rs` → `docs/archive/CHG-070/desktop-retired-tree/engine/pty.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob b17874492947c3ba83ce50bd41e493c499ec5337

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/engine/pty.rs"
  ],
  "testTargets": [
    "packages/remote/src/shared/terminal/ptyBridge.test.ts",
    "scripts/cdp-pty-state.test.mjs",
    "src/lib/terminal/ptyRuntimeSnapshot.test.ts",
    "src/lib/terminal/ptyWriteQueue.test.ts"
  ]
}
```
