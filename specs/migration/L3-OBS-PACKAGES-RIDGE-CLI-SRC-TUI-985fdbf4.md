---
id: L3-OBS-PACKAGES-RIDGE-CLI-SRC-TUI-985fdbf4
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/ridge-cli/src/tui module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-cli/src/tui/dashboard.rs
  - packages/ridge-cli/src/tui/keymap.rs
  - packages/ridge-cli/src/tui/lan_proto.rs
  - packages/ridge-cli/src/tui/lan_session.rs
  - packages/ridge-cli/src/tui/mod.rs
  - packages/ridge-cli/src/tui/pager.rs
  - packages/ridge-cli/src/tui/qr_display.rs
  - packages/ridge-cli/src/tui/session.rs
test_targets: []
---

# packages/ridge-cli/src/tui module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`packages/ridge-cli/src/tui/lan_host_impl.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/tui/lan_host_impl.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 4495c07846c390e1e895662e8bb1bee6f562877b

`packages/ridge-cli/src/tui/lan_host.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/tui/lan_host.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 21bf57c48a8f624f69b495f8ff16ccdcbe881946

`packages/ridge-cli/src/tui/scrollback.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/tui/scrollback.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob ca1aa593e112fd612ba5300ccdf54c5a0766b205

`packages/ridge-cli/src/tui/workspace.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/tui/workspace.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 155dd48292448423a553bb8f9da114c6cc118e51

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "packages/ridge-cli/src/tui/dashboard.rs",
    "packages/ridge-cli/src/tui/keymap.rs",
    "packages/ridge-cli/src/tui/lan_host_impl.rs",
    "packages/ridge-cli/src/tui/lan_host.rs",
    "packages/ridge-cli/src/tui/lan_proto.rs",
    "packages/ridge-cli/src/tui/lan_session.rs",
    "packages/ridge-cli/src/tui/mod.rs",
    "packages/ridge-cli/src/tui/pager.rs",
    "packages/ridge-cli/src/tui/qr_display.rs",
    "packages/ridge-cli/src/tui/scrollback.rs",
    "packages/ridge-cli/src/tui/session.rs",
    "packages/ridge-cli/src/tui/workspace.rs"
  ],
  "testTargets": []
}
```
