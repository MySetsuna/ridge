# Wave53 交接：Host 权威布局同步与 CWD 围栏

日期：2026-08-10

## 本波落地

- `src/lib/stores/paneTree.coverage.test.ts` 新增后端布局同步回归，验证 Host 返回 workspace id 覆盖本地旧值、active pane 重选、旧 CWD 删除、新 pane CWD seed、跨 workspace CWD 保留，以及 Host id 查询失败回退。
- 未改变布局同步生产逻辑；本波仅补确定性覆盖，未向 Codex 之外 CLI、agent 或 teammate 发消息，未推送、未发布。

## 验证

- `pnpm exec vitest run src/lib/stores/paneTree.coverage.test.ts`：11/11 通过。
- `pnpm test:coverage:sonar`：exit `0`；V8/LCOV statements `12774/18608 = 68.64%`，branches `7066/11610 = 60.86%`，functions `2492/3536 = 70.47%`，lines `11514/15895 = 72.43%`；本地 statements 达到 80% 尚缺 `2113` 条。
- `pnpm check`：`0 errors / 0 warnings`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。

## 未闭环与交接

- Sonar project 实际 coverage `>=80%`、Quality Gate `OK` 仍无新服务器证据；本地 LCOV 不替代 Sonar 指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`；PTY 五条件原子运行时现场证据、第三方 Runtime/A2A 私有协议兼容性仍待外部证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改未纳入提交。
