---
id: L4-OBS-SRC-TAURI-SRC-REMOTE-HOST-IMPL-RS-5d15687d
level: L4
parent: L3-OBS-SRC-TAURI-SRC-bcb33161
title: remote_host_impl.rs
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-090/source/src-tauri/src/remote_host_impl.rs
test_targets: []
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# remote_host_impl.rs

提供單一桌面 shell 端口 dispatch_remote_invoke(method, args)，復用既有 dispatch_invoke_jsonrpc。core-migrated 命令進 ridge_core::dispatch，legacy 命令仍受同一 remote allowlist 約束。

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`src-tauri/src/remote_host_impl.rs` → `docs/archive/CHG-090/source/src-tauri/src/remote_host_impl.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 4494aeb7dec320fa924d5ddcb45ff6e29705efb1

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src-tauri/src/remote_host_impl.rs"
  ],
  "testTargets": []
}
```
