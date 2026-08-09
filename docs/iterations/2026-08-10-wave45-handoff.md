# Wave45 交接：Cloud Host store scope 与事件投影覆盖

日期：2026-08-10

## 本波落地

- `src/lib/remote/cloud/cloudHostStore.test.ts` 新增浏览器凭据缺失与 host 启动失败、host 状态/session/error 回调、普通/共享 workspace bridge 配置、workspace invoke 拒绝与 pane 事件 workspace 过滤测试。
- 测试通过真实 store 回调闭环检查 `set_cloud_remote_active`、`get_pane_layout_for`、workspace scope deny 与事件 unsubscribe；未改生产逻辑。

## 验证

- 局部通信回归：`52/52` 通过。
- `pnpm test:coverage:sonar`：`199` files；`1837 passed / 1 skipped`；statements `12591/18603 = 67.68%`；branches `6937/11608 = 59.76%`；functions `2462/3536 = 69.62%`；lines `11364/15891 = 71.51%`。
- 本地 statements 80% 仍缺 `2292` 条；部分 `.mjs` 仍有 `PARSE_ERROR/Expected ident`，不宣称 Sonar 80% 或 Quality Gate 通过。

## 未闭环

- Sonar project `>=80%`、Quality Gate、scanner/CE 成功证据仍 ACTIVE。
- PTY 真实五条件原子采样注入、第三方 Runtime/A2A 协议兼容仍缺现场/协议证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态与用户既有 CDP 改动不纳入提交；未 push/tag/release。
