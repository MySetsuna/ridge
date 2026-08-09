# Wave64 交接：wsRemote Agent Hub 与滚动提交失败边界

日期：2026-08-10

## 本波落地

- 在 `packages/remote/src/shared/transport/wsRemote.behavior.test.ts` 补齐 Agent Hub receipt 边界：空结果视为 malformed receipt；结构化 `STALE_LEASE` 错误与 topology 错误原样转为可观察 rejection。
- 补齐 scrollback 提交边界：无 cursor 短路、空页与 `atOldest` malformed page fail-closed、结束序列错配拒绝、有效页在 stale discard 后不得提交。
- 仅新增确定性测试；未改生产通信语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 证据

- 聚焦：`pnpm exec vitest run packages/remote/src/shared/transport/wsRemote.behavior.test.ts`，`6/6` 通过。
- 全量：`pnpm test:coverage:sonar` exit `0`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。
- 本地 V8/LCOV：statements `13210/18608 = 70.99%`；branches `7268/11610 = 62.60%`；functions `2568/3536 = 72.62%`；lines `11907/15895 = 74.91%`；距 statements 80% 尚缺 `1677` 条。
- 静态检查：`pnpm check`，`0 errors / 0 warnings`。

## 未闭环与交接

- `REQ-SONAR-COVERAGE-80-01` 仍 `ACTIVE`：以上为本地 LCOV，不是 Sonar project coverage 或 Quality Gate 证据；真实 Sonar 扫描、project coverage `>=80%`、Quality Gate `OK` 仍待可用凭证/服务端结果。
- PTY 五条件原子运行时快照、第三方 Runtime/A2A 真实兼容性、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限、移动 profile 与现场 Remote 四路径仍待对应证据。
- `.mjs` coverage 的 `PARSE_ERROR/Expected ident` 仍须单独治理，不得通过扩大排除范围伪造覆盖率。
- 本波运行态 `coverage/*`、`.iteration/*` 与 NotebookLM 运行态未纳入提交。
