---
id: L4-OBS-PACKAGES-RIDGE-CLI-SRC-PTY-RS-c81f693e
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CLI-SRC-353b0498
title: pty.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-cli/src/pty.rs
test_targets:
  - packages/remote/src/shared/terminal/ptyBridge.test.ts
  - scripts/cdp-pty-state.test.mjs
  - src/lib/terminal/ptyWriteQueue.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PTYBRIDGE-TEST-TS-3f30eb8e
  - TEST-OBS-SCRIPTS-CDP-PTY-STATE-TEST-MJS-462b9d95
  - TEST-OBS-SRC-LIB-TERMINAL-PTYRUNTIMESNAPSHOT-TEST-TS-a223e946
  - TEST-OBS-SRC-LIB-TERMINAL-PTYWRITEQUEUE-TEST-TS-b49b9e91
---

# pty.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置 — 测试引用

旧测试已随 CHG-070 源链迁出。保留原始测试映射及 Git 归档证据，移除现役失效引用。其余原有测试目标保留，不按名称猜测替代覆盖。

原始 test_targets

```json
[
  "packages/remote/src/shared/terminal/ptyBridge.test.ts",
  "scripts/cdp-pty-state.test.mjs",
  "src/lib/terminal/ptyRuntimeSnapshot.test.ts",
  "src/lib/terminal/ptyWriteQueue.test.ts"
]
```

`src/lib/terminal/ptyRuntimeSnapshot.test.ts` → `docs/archive/CHG-090/source/src/lib/terminal/ptyRuntimeSnapshot.test.ts`
