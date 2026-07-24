# CONTRACT — Iteration 46 / AC4-C7（约 2 日 · HITL remote audit）

**Credit ID**: C7 · **NEW**

## 产品结果

脱敏审批历史环；`list_hitl_audit_remote` 入 REMOTE_ALLOWLIST + remote host 路由；裁决写 audit。

## 独占主文件

- `src-tauri/src/teammate/hitl_audit.rs`
- capability.rs / remoteAllowlist / capabilityContract / remote_host_impl
- hitl.rs record_decision → audit

## 验收

`cargo test -p ridge --lib hitl_audit` + vitest capabilityContract → gates-credit-C7.log
