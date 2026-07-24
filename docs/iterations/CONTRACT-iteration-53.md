# CONTRACT — Iteration 53 / AC4-C53（约 2 日 · HITL 审计过滤脱敏全链）

**Credit ID**: C53 · OP-AGENT-CP 加厚

## 产品结果
hitlAuditFilter 脱敏/过滤/时间线；Rust redact_reason_summary + list_audit_filtered + audit_verdict_counts；resolve 路径 append_audit；AgentCenter 过滤展示。

## 独占主文件
- `src/lib/teammate/hitlAuditFilter.ts`(+test)
- `src-tauri/src/teammate/hitl_audit.rs`
- `src/lib/teammate/AgentCenterPanel.svelte`
- `packages/remote/.../hitlAuditRemote.ts`

## 验收
vitest hitlAuditFilter；cargo hitl_audit。

## 停机
远程投影命令全文。
