# Wave68 交接：wsRemote 能力降级与鉴权关闭边界覆盖

日期：2026-08-10

## 本波落地

- 覆盖能力公告的去重与非字符串过滤、unsupported method 后能力降级。
- 覆盖空 pane/空 stdin 输入、坏 pty-meta/pty-resized/scrollback-meta 输入及断开后请求超时。
- 覆盖已认证 WebSocket `4403` + `DEVICE_PARKED` 的不可重试错误分级。
- 仅新增确定性测试；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 证据

- 聚焦：`pnpm exec vitest run packages/remote/src/shared/transport/wsRemote.behavior.test.ts`，`7/7` 通过。
- 全量：`pnpm test:coverage:sonar` exit `0`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。
- 本地 V8/LCOV：statements `13284/18608 = 71.38%`；branches `7318/11610 = 63.03%`；functions `2585/3536 = 73.10%`；lines `11960/15895 = 75.24%`；距 statements 80% 尚缺 `1603` 条。
- 静态检查：`pnpm check`，`0 errors / 0 warnings`。

## 未闭环与交接

- `REQ-SONAR-COVERAGE-80-01` 仍 `ACTIVE`：本地 LCOV 不等价于 Sonar project coverage；真实扫描、project coverage `>=80%`、Quality Gate `OK` 仍待服务端证据。
- PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres、物理 DPR、跨卷权限、移动 profile 与现场 Remote 四路径仍待证据。
- `.mjs` coverage `PARSE_ERROR/Expected ident` 仍须治理，不扩大排除范围。
- `coverage/*`、`.iteration/*` 与 NotebookLM 运行态未纳入本波提交。
