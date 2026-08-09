# Wave59 交接：Cloud HostBridge admission 与信任门控边界

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/cloud/cloudHostBridge.test.ts` 新增非对象 CONTROL/JSON/未知通道 fail-closed、带 id 的 `$/hello`/`$/cancel`、协议拒绝与非白名单拒绝、notification invoke 失败、TOTP trust malformed/lockout、信任记录持久化失败回归。
- 聚焦 `cloudHostBridge.test.ts`：`64/64` 通过；未改 HostBridge 生产运行语义，未向 Codex 之外 CLI、agent 或 teammate 发消息。

## 验证

- `pnpm check`：`0 errors / 0 warnings`。
- `pnpm test:coverage:sonar`：exit `0`；`scripts/normalize-lcov.mjs`：`ok=true`。
- 本地 V8/LCOV：statements `13011/18608 = 69.92%`、branches `7154/11610 = 61.61%`、functions `2539/3536 = 71.80%`、lines `11735/15895 = 73.82%`；距 statements 80% 尚缺 `1876` 条。

## 未闭环与交接

- Sonar project 实际 coverage `>=80%` 与 Quality Gate `OK` 仍须真实 Sonar 服务证据；本地 LCOV 不冒充 Sonar 指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`；PTY 五条件原子运行时、第三方 Runtime/A2A 兼容性、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动端 profile 仍属在途需求。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改未纳入本次提交。
