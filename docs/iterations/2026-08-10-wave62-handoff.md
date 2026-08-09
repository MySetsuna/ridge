# Wave62 交接：TerminalManager 渲染顺序与尺寸调度覆盖

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/terminal/manager.test.ts` 新增多 pane RAF 渲染顺序回归：focused pane 优先、非 focused pane 轮转、公平性及 parked pane 过滤。
- 新增 viewport resize trailing-fit 回归：重复 resize 合并、500ms settle 后单次 fit、parked/missing pane fail-safe。
- 未改生产语义；未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 验证

- 聚焦：`pnpm exec vitest run packages/remote/src/shared/terminal/manager.test.ts packages/remote/src/shared/terminal/manager.attach.test.ts`，`22/22` 通过。
- 全量：`pnpm test:coverage:sonar` exit `0`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。
- 本地 V8/LCOV：statements `13185/18608 = 70.85%`，branches `7250/11610 = 62.44%`，functions `2558/3536 = 72.34%`，lines `11887/15895 = 74.78%`；距 statements 80% 尚缺 `1702` 条。
- 静态检查：`pnpm check` 为 `0 errors / 0 warnings`。

## 未闭环与交接

- `REQ-SONAR-COVERAGE-80-01` 仍 ACTIVE：尚无真实 Sonar `>=80%` 与 Quality Gate `OK` 证据；本地 LCOV 不冒充 Sonar 指标。
- PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限、移动 profile 仍待对应现场证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 产物保持未纳入本次提交。
