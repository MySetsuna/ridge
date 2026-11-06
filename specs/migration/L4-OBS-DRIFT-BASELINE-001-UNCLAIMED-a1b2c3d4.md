---
id: L4-OBS-DRIFT-BASELINE-001-UNCLAIMED-a1b2c3d4
level: L4
parent: L3-OBS-DRIFT-BASELINE-001-UNCLAIMED-a1b2c3d4
title: BASELINE-001 unclaimed drift paths
status: LOCKED
origin: drift
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/operations/remote-cloud-release.md
  - static/fonts/fonts.css
  - static/fonts/inter-italic-latin.woff2
  - static/fonts/inter-normal-latin.woff2
  - static/fonts/jetbrainsmono-italic-latin.woff2
  - static/fonts/jetbrainsmono-normal-latin.woff2
  - packages/ridge-cli/src/session_control.rs
  - src/lib/monaco/workers.ts
test_targets: []
---

Pre-existing project files that were never explicitly claimed by any CHG's codeTargets. Absorbed under CHG-054 so the lock can close without expanding product scope.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`packages/ridge-cli/src/session_control_tests.rs` → `docs/archive/CHG-090/source/packages/ridge-cli/src/session_control_tests.rs`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob eabcd2f1ba539adae729df1b2f31bef098e1523e

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "docs/operations/remote-cloud-release.md",
    "static/fonts/fonts.css",
    "static/fonts/inter-italic-latin.woff2",
    "static/fonts/inter-normal-latin.woff2",
    "static/fonts/jetbrainsmono-italic-latin.woff2",
    "static/fonts/jetbrainsmono-normal-latin.woff2",
    "packages/ridge-cli/src/session_control.rs",
    "packages/ridge-cli/src/session_control_tests.rs",
    "src/lib/monaco/workers.ts"
  ],
  "testTargets": []
}
```
