---
id: L4-OBS-SRC-TAURI-SRC-REMOTE-HOST-IMPL-RS-5d15687d
level: L4
parent: L3-OBS-SRC-TAURI-SRC-bcb33161
title: remote_host_impl.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/remote_host_impl.rs
---

# remote_host_impl.rs

提供單一桌面 shell 端口 dispatch_remote_invoke(method, args)，復用既有 dispatch_invoke_jsonrpc。core-migrated 命令進 ridge_core::dispatch，legacy 命令仍受同一 remote allowlist 約束。
