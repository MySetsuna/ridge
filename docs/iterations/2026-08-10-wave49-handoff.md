# Wave49 交接：Cloud Host bridge 重连清理围栏

日期：2026-08-10

## 本波落地

- `CloudHostBridge.reset()` 现在显式退订并清空 DataChannel 背压监听，避免旧连接 `bufferedamountlow` 回调跨重连残留。
- `packages/remote/src/shared/cloud/cloudHostBridge.test.ts` 覆盖 host event verified 门控/退订、preauthorized connected 回执、背压控制替换与 reset 清理。
- 该修复保持 PTY/控制通道协议不变；未向 Codex 外 CLI/Agent 发消息。

## 验证

- Host bridge 聚焦：`60/60`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV：statements `12723/18606 = 68.38%`；branches `7030/11608 = 60.56%`；functions `2486/3536 = 70.30%`；lines `11473/15894 = 72.18%`。
- 本地 statements 80% 目标需 `14885` 条，当前尚缺 `2162` 条。
- `pnpm check`：`0 errors / 0 warnings`。

## 未闭环

- Sonar project coverage `>=80%`、Quality Gate OK、scanner/CE 成功证据仍未闭环；本地 LCOV 不替代 Sonar 项目指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`，需后续修复可测化/解析边界，不以排除代码冒充覆盖率。
- PTY 五条件原子运行时采样、第三方 Runtime/A2A 私有协议兼容仍需现场或等价证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改继续保留 dirty，不纳入提交。
