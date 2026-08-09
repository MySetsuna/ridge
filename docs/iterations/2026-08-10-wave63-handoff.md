# Wave63 交接：CloudRemote 降级与滚动历史边界

日期：2026-08-10

## 本波落地

- `src/remote/lib/cloudRemote.test.ts` 新增可选 Cloud parity 命令失败回归：workspace 查询、Agent message/session、保存/打开/关闭 workspace、theme catalog、layout 查询均验证 fail-safe 或保留错误语义。
- 新增 resync-frame 不可用时回退 bounded tail、实时 PTY 先后顺序、滚动历史 cursor commit/at-oldest 及无 cursor 短路回归。
- 新增 provider error 分类与未授权后状态边界；未改生产语义，未向 Codex 之外 CLI、Agent 或 teammate 发消息。

## 验证

- 聚焦：`pnpm exec vitest run src/remote/lib/cloudRemote.test.ts`，`52/52` 通过。
- 全量：`pnpm test:coverage:sonar` exit `0`；`scripts/normalize-lcov.mjs` 返回 `ok=true`。
- 本地 V8/LCOV：statements `13203/18608 = 70.95%`，branches `7258/11610 = 62.51%`，functions `2568/3536 = 72.62%`，lines `11902/15895 = 74.87%`；距 statements 80% 尚缺 `1684` 条。
- 静态检查：`pnpm check` 为 `0 errors / 0 warnings`。

## 未闭环与交接

- `REQ-SONAR-COVERAGE-80-01` 仍 ACTIVE：尚无真实 Sonar `>=80%` 与 Quality Gate `OK` 证据；本地 LCOV 不冒充 Sonar 指标。
- PTY 五条件原子运行时、第三方 Runtime/A2A、Cloud/Postgres 真 E2E、物理 DPR、跨卷权限、移动 profile 仍待对应现场证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 产物保持未纳入本次提交。
