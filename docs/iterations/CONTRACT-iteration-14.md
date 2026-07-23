# CONTRACT — iteration 14（存量终轮）

日期：2026-07-23
主题：M1 切片二 + M2 合并——HITL 裁决 decisions 持久化 + agent 归因（「上下文持久化增强」，iteration 11 初裁存照之合并轮）

## 自动轨目标

### G1[M1/M2·主线] decisions 持久化 + 归因

- 新 `teammate/memory.rs`：workspace-memory sidecar 的**唯一** doc 级读改写（进程互斥 + 原子写；文件删除条件 = 无 suspendedPanes 且无 decisions）；`DIR` OnceLock 于 lib.rs setup 注入（suspend.rs 持久化改走同一 dir 源，删双解析路径）。
- `PendingEntry` += `wid`（`request_approval` 签名 += wid，唯一调用方 route_send_keys 已持有）；三消费点写 decision：桌面 `resolve`、远端 `resolve_remote`（含败者尝试 already-resolved/nonce-mismatch 亦记）、超时 fail-closed。条目 `{ts, source: desktop|remote|timeout, initiator, verdict, riskLevel, reasonSummary, outcome}`——**绝不含命令全文**；环形上限 50。
- **M2 归因**：route_send_keys 的 initiator 由 `pane#{idx}` 升级为稳定 `agent_id`（`teammate_agent_pane_map` 反查，无则回落 pane 号）——审批/裁决事件归因到 agent 身份。
- 读方（真实消费者）：桌面命令 `list_hitl_decisions(workspace_id)`（仅桌面 IPC，不入 REMOTE_ALLOWLIST）+ Agent Center 「审批历史」区展示。
- 验收：`cargo test --workspace` exit 0；memory.rs 测试（RMW 保留他节/cap 50/空 doc 删文件）；决策链测试（consume → decision 落盘，含 initiator 归因）；投影/负断言维持。
- 停机：无（IO 沿 fail-open 纪律）。
- 减法：不做 decisions 远端暴露（需宣告纪律，待需求）；不做 goal/tasks 切片三。

### G2 门禁与闭环

- 全门禁绿 + 导读刷新 + 循环闭幕（此轮毕，自动轨存量**真尽**——含解冻后余项）。

## 自动验收

```powershell
cargo test --workspace
pnpm exec vitest run packages/remote/src/shared/
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
node scripts/generate-review-pack.mjs
```

## 明确不做

远端读 decisions；M1 切片三（goal/constraints/tasks UI）；G1 阶段二（待痛点证据裁决维持）。
