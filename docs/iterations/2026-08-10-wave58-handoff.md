# Wave58 交接：Controller Cloud 重连与绑定终止边界

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/cloud/controllerCloudProvider.test.ts` 新增信令 error/close、offer 创建失败、pane lane 关闭后回落 control lane、ArrayBufferView 入站及 E2EE 绑定宽限期后 relay-trust 回落回归。
- 聚焦 `controllerCloudProvider.test.ts`：`28/28` 通过；未改 WebRTC/信令生产语义，未向 Codex 之外 CLI、agent 或 teammate 发消息。

## 验证

- `pnpm check`：`0 errors / 0 warnings`。
- `pnpm test:coverage:sonar`：exit `0`；`scripts/normalize-lcov.mjs`：`ok=true`。
- 本地 V8/LCOV：statements `12981/18608 = 69.76%`、branches `7140/11610 = 61.49%`、functions `2537/3536 = 71.74%`、lines `11705/15895 = 73.63%`；距 statements 80% 尚缺 `1907` 条。

## 未闭环与交接

- Sonar project 实际 coverage `>=80%` 与 Quality Gate `OK` 仍须真实 Sonar 服务证据；本地 LCOV 不冒充 Sonar 指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`；PTY 五条件原子运行时、第三方 Runtime/A2A 兼容性、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动端 profile 仍属在途需求。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改未纳入本次提交。
