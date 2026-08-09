# Wave51 交接：Cloud Host bridge pane 订阅与背压恢复边界

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/cloud/cloudHostBridge.test.ts` 补齐无效 pane 订阅、pane source 缺失退订句柄、超长 pane id 编码失败、背压后切换 active 的私有 resync 恢复。
- 继续验证 Host bridge 对 TOTP、pane、背压与重连边界的 fail-closed 行为；未向 Codex 外 CLI/Agent 发消息。

## 验证

- Host bridge 聚焦：`62/62`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV：statements `12743/18606 = 68.48%`；branches `7044/11608 = 60.68%`；functions `2490/3536 = 70.41%`；lines `11486/15894 = 72.26%`。
- 本地 statements 80% 目标需 `14885` 条，当前尚缺 `2142` 条。
- `pnpm check`：`0 errors / 0 warnings`。

## 未闭环

- Sonar project coverage `>=80%`、Quality Gate OK、scanner/CE 成功证据仍未闭环；本地 LCOV 不替代 Sonar 项目指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`，需后续修复可测化/解析边界，不以排除代码冒充覆盖率。
- PTY 五条件原子运行时采样、第三方 Runtime/A2A 私有协议兼容仍需现场或等价证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改继续保留 dirty，不纳入提交。
