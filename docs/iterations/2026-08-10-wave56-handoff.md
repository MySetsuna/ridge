# Wave56 交接：Cloud API/认证 cookie 边界与覆盖补强

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/cloud/apiClient.test.ts` 覆盖统一 JSON envelope 下的账户、workspace share、SSO cookie、device-code、password、ICE 等公开 API 路由；验证 Bearer header、`credentials: include`、路径编码、401 仅重试一次、网络/未知错误码/坏 JSON fail-closed。
- `packages/remote/src/shared/cloud/auth.test.ts` 覆盖浏览器授权 pending → wake → approved、expired、设备绑定补齐 username、畸形 localStorage 用户数据；对应临时来源提出的 cookie/API、取消/重试边界。
- 未改 Cloud API/认证生产语义，未向 Codex 之外 CLI、agent 或 teammate 发消息。

## 验证

- 聚焦 Cloud API/认证：`16/16` 通过。
- `pnpm check`：`0 errors / 0 warnings`。
- `pnpm test:coverage:sonar`：exit `0`；`scripts/normalize-lcov.mjs`：`ok=true`。
- 本地 V8/LCOV：statements `12947/18608 = 69.57%`、branches `7119/11610 = 61.31%`、functions `2533/3536 = 71.63%`、lines `11675/15895 = 73.45%`；距 statements 80% 尚缺 `1940` 条。

## 未闭环与交接

- Sonar project 实际 coverage `>=80%` 与 Quality Gate `OK` 仍须真实 Sonar 服务证据；本地 LCOV 不冒充 Sonar 指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`；PTY 五条件原子运行时、第三方 Runtime/A2A 兼容性、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限及移动端 profile 仍属在途需求。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改未纳入本次提交。
