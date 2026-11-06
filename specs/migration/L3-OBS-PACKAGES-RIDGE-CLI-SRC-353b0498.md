---
id: L3-OBS-PACKAGES-RIDGE-CLI-SRC-353b0498
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/ridge-cli/src module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-cli/src/batching.rs
  - packages/ridge-cli/src/config.rs
  - packages/ridge-cli/src/core_host.rs
  - packages/ridge-cli/src/daemon.rs
  - packages/ridge-cli/src/e2ee.rs
  - packages/ridge-cli/src/envelope.rs
  - packages/ridge-cli/src/fs_reuse.rs
  - packages/ridge-cli/src/host.rs
  - packages/ridge-cli/src/ice.rs
  - packages/ridge-cli/src/kernel_ctl.rs
  - packages/ridge-cli/src/kernel_host_impl.rs
  - packages/ridge-cli/src/key_binding.rs
  - packages/ridge-cli/src/login_flow.rs
  - packages/ridge-cli/src/main.rs
  - packages/ridge-cli/src/mux.rs
  - packages/ridge-cli/src/protocol.rs
  - packages/ridge-cli/src/pty.rs
  - packages/ridge-cli/src/rpc.rs
  - packages/ridge-cli/src/rtc.rs
  - packages/ridge-cli/src/signaling.rs
  - packages/ridge-cli/src/totp.rs
test_targets: []
---

# packages/ridge-cli/src module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`packages/ridge-cli/src/daemon_ctl.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/daemon_ctl.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 32e24ff695afe625da0c51c1bc6d6f7b3fba5f3f

`packages/ridge-cli/src/session.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/session.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob c59ff2a3b42c42d4b999f845475ed8b933776587

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "packages/ridge-cli/src/batching.rs",
    "packages/ridge-cli/src/config.rs",
    "packages/ridge-cli/src/core_host.rs",
    "packages/ridge-cli/src/daemon_ctl.rs",
    "packages/ridge-cli/src/daemon.rs",
    "packages/ridge-cli/src/e2ee.rs",
    "packages/ridge-cli/src/envelope.rs",
    "packages/ridge-cli/src/fs_reuse.rs",
    "packages/ridge-cli/src/host.rs",
    "packages/ridge-cli/src/ice.rs",
    "packages/ridge-cli/src/kernel_ctl.rs",
    "packages/ridge-cli/src/kernel_host_impl.rs",
    "packages/ridge-cli/src/key_binding.rs",
    "packages/ridge-cli/src/login_flow.rs",
    "packages/ridge-cli/src/main.rs",
    "packages/ridge-cli/src/mux.rs",
    "packages/ridge-cli/src/protocol.rs",
    "packages/ridge-cli/src/pty.rs",
    "packages/ridge-cli/src/rpc.rs",
    "packages/ridge-cli/src/rtc.rs",
    "packages/ridge-cli/src/session.rs",
    "packages/ridge-cli/src/signaling.rs",
    "packages/ridge-cli/src/totp.rs"
  ],
  "testTargets": []
}
```
