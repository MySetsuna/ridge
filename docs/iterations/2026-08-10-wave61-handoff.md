# Wave61 交接：paneTree 分屏状态机与保存回退覆盖

日期：2026-08-10

## 本波落地

- `src/lib/stores/paneTree.coverage.test.ts` 新增分屏拖拽状态机回归：`pending -> drag -> idle`、重复引用去重、ratio 更新、release 返回值与空闲释放。
- 新增非桌面 persistence/startup fallback 回归；覆盖保存工作区、启动上下文、最近/恢复集合、已保存文件列表在 Tauri 不可用、成功及可选命令失败时的 fail-safe 行为。
- 未改生产语义；未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 验证

- 聚焦：`pnpm exec vitest run src/lib/stores/paneTree.coverage.test.ts`，`13/13` 通过。
- 全量：`pnpm test:coverage:sonar` exit `0`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。
- 本地 V8/LCOV：statements `13090/18608 = 70.34%`，branches `7192/11610 = 61.94%`，functions `2550/3536 = 72.11%`，lines `11799/15895 = 74.23%`；距 statements 80% 尚缺 `1797` 条。
- 静态检查：`pnpm check` 为 `0 errors / 0 warnings`。

## 未闭环与交接

- `REQ-SONAR-COVERAGE-80-01` 仍 ACTIVE：尚无真实 Sonar `>=80%` 与 Quality Gate `OK` 证据；本地 LCOV 不冒充 Sonar 指标。
- PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限、移动 profile 仍待对应现场证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 产物保持未纳入本次提交。
