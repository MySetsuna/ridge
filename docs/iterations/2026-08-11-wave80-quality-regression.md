# Wave 80：质量回归边界补强（2026-08-11）

## 代码与测试

- `packages/remote/src/shared/hosts/foreignPaneStatus.ts`：订阅恢复状态的重连次数统一采用非负值，避免异常输入把负数泄露到 UI 状态。
- `packages/remote/src/shared/hosts/foreignPaneStatus.test.ts`：补齐 connecting、未订阅及负重连次数回归。
- `packages/remote/src/shared/terminal/paneInputGate.test.ts`：补齐有界意图队列满时 `PaneInputGateFullError` 的结构化断言。
- 聚焦测试：2 files / 13 passed / 0 failed。
- 前端全量覆盖率测试：215 files / 1980 passed / 1 skipped；coverage 命令 exit `0`。
- `pnpm check`：0 errors / 0 warnings。

本地 LCOV 当前为 statements `66.02%`、branches `60.18%`、functions `68.27%`、lines `70.11%`。该报告包含本地配置范围，不能替代 Sonar 项目级指标。

## 构建

- `pnpm build:remote:mobile`：exit `0`，PWA 生成 38 个 precache entries。
- `pnpm verify:pwa`：exit `0`；viewport、manifest standalone/scope/icons、service worker、safe-area CSS 与无站内安装钩子均通过。
- 首次 `pnpm build:remote:desktop` 曾因现场内存不足以 `4294967295` 中止；资源恢复后复跑通过，最终 desktop SSR/client 构建 exit `0`，产物写入 `remote-dist/desktop`，日志为 `.tools/wave81-build-remote-desktop.log`。

## Sonar

最近一次成功上传的项目扫描仍为 `c271e74b-ac3f-4277-bbef-74418f48b822`：新覆盖率 `80.1%`、新重复率 `0.84181%` 均通过，Quality Gate 因旧扫描产生的 `rust:S3776` 新问题失败。该问题已由提交 `254a98e3` 重构；本轮未能复扫，因为当前 Sonar 登录校验返回 `401` / `valid=false`。未猜测或保存凭据，故不宣称 Gate 已恢复。

## 提交

本波代码、测试与证据文档应作为同一质量补强批次提交；不包含 `.iteration`、coverage 或 `.tools` 运行态产物。
