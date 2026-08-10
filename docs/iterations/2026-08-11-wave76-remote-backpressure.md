# Wave 76：Remote feed 背压与调度生命周期（2026-08-11）

## 变更范围

本波针对 `REQ-MOBILE-REMOTE-BACKPRESSURE-01`，补齐远端 pane feed 的有限时间预算与调度句柄生命周期：

- `TerminalManager.flushPaneFeed` 不再接受 `Infinity` 作为无限制刷新预算；输入预算统一归一到有限的 `MAX_PANE_FEED_FLUSH_BUDGET_MS`。
- `PaneFeedScheduler` 对队列容量、单帧预算、单帧字节数及步长做有限值校验；非有限输入回退到安全默认值。
- scheduler 保存真实 RAF/timeout 句柄；`drainNow`、`clear`、`clearAll`、`dispose` 会取消尚未执行的调度，避免清空后旧回调继续消费或重生队列。
- 新增取消句柄、清空队列、非有限配置及有限预算回归测试。

## 确定性证据

- 定向 Remote feed：`3 files / 31 passed`。
- 全量 Vitest：`215 files / 1978 passed / 1 skipped`。
- `pnpm check`：`0 errors / 0 warnings`。
- `pnpm test:coverage:sonar`：exit `0`；本地 LCOV statements `14476/21935 = 65.99%`、branches `8054/13387 = 60.16%`、functions `2842/4167 = 68.20%`、lines `12983/18523 = 70.09%`。此为本地 V8 基线，不替代 Sonar 项目指标。

日志：`.tools/verify-remote-backpressure-wave76.log`、`.tools/verify-full-test-wave76.log`、`.tools/verify-check-wave76.log`、`.tools/verify-coverage-wave76.log`。

## 质量门边界

- 当前本波未成功上传新的 Sonar 分析；此前权威项目 API 分析 `9abdb231-3503-4f6e-b8ee-48d28f7086dc` 为 coverage `80.4%`、line `86.7%`、branch `71.5%`、Quality Gate `OK`、`new_violations=0`、`new_issues=0`。
- Sonar 项目总览仍记录 1 条历史 `Web:PageWithoutTitleCheck`；当前 `src/remote/index.html` 已有 `<title>`，须成功上传新分析后再确认历史 issue 是否刷新清零。
- `pnpm build` 本波在 180 秒墙钟上限内停留于 Vite SSR transforming，已清理该次构建进程树；不能据此宣称构建通过。Tauri 全量复核亦未形成本波新绿证据。

本波提交：`6639e1fe`（`fix bound remote pane feed scheduling`）。生成的 coverage、`.iteration`、E2E 运行产物未纳入提交。
