# Wave50 交接：LAN RemoteConnection 前台与缓存边界覆盖

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/transport/wsRemote.behavior.test.ts` 补齐坏 JSON fail-closed、缺 workspace 输出丢弃、5000 行输出缓存上限、visibility 前台 liveness probe 与 disconnect listener 清理。
- 该波仅增确定性测试；未改变 LAN WS 协议、重连或缓存生产语义，未向 Codex 外 CLI/Agent 发消息。

## 验证

- RemoteConnection 聚焦（含 scheduler 回归）：`25/25`。
- 全量 `pnpm test:coverage:sonar` exit `0`；本地 V8/LCOV：statements `12735/18606 = 68.44%`；branches `7041/11608 = 60.65%`；functions `2488/3536 = 70.36%`；lines `11480/15894 = 72.22%`。
- 本地 statements 80% 目标需 `14885` 条，当前尚缺 `2150` 条。
- `pnpm check`：`0 errors / 0 warnings`。

## 未闭环

- Sonar project coverage `>=80%`、Quality Gate OK、scanner/CE 成功证据仍未闭环；本地 LCOV 不替代 Sonar 项目指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`，需后续修复可测化/解析边界，不以排除代码冒充覆盖率。
- PTY 五条件原子运行时采样、第三方 Runtime/A2A 私有协议兼容仍需现场或等价证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改继续保留 dirty，不纳入提交。
