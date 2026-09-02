---
id: L4-OBS-SRC-TAURI-SRC-LIB-RS-eabcebd0
level: L4
parent: L3-OBS-SRC-TAURI-SRC-bcb33161
title: lib.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/lib.rs
---

# lib.rs

註冊 dispatch_remote_invoke 作桌面 shell 的通用 Remote RPC 入口；不得暴露 host-only command，實際 admission 仍由既有 remote dispatcher 與 ridge-core capability gate 決定。
