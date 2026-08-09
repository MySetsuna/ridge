# Wave66 交接：ridgeCloudProvider 会话与信令生命周期覆盖

日期：2026-08-10

## 本波落地

- 扩展 `ridgeCloudProvider.test.ts` 测试夹具，记录 session snapshot 与 channel backpressure 控制回调。
- 覆盖 ICE candidate 定向信令、控制通道 drain/取消、Pane 通道关闭、坏入站业务帧、握手后等待信令公钥再建桥。
- 覆盖信令断线后的有界退避重连及 WebSocket 构造异常的 error 状态。
- 仅新增确定性测试；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 证据

- 聚焦：`pnpm exec vitest run packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts`，`19/19` 通过。
- 全量：`pnpm test:coverage:sonar` exit `0`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。
- 本地 V8/LCOV：statements `13270/18608 = 71.31%`；branches `7305/11610 = 62.91%`；functions `2582/3536 = 73.02%`；lines `11953/15895 = 75.19%`；距 statements 80% 尚缺 `1617` 条。
- 静态检查：`pnpm check`，`0 errors / 0 warnings`。

## 未闭环与交接

- `REQ-SONAR-COVERAGE-80-01` 仍 `ACTIVE`：本地 LCOV 不等价于 Sonar project coverage；真实扫描、project coverage `>=80%`、Quality Gate `OK` 仍待服务端证据。
- PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres、物理 DPR、跨卷权限、移动 profile 与现场 Remote 四路径仍待证据。
- `.mjs` coverage `PARSE_ERROR/Expected ident` 仍须治理，不扩大排除范围。
- `coverage/*`、`.iteration/*` 与 NotebookLM 运行态未纳入本波提交。
