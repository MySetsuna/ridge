# Wave44 交接：Agent/Cloud 通信边界与编组持久化覆盖

日期：2026-08-10

## 本波落地

- `packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts` 新增 Cloud Host 边界回归：ICE 获取失败、重复上线与信令断线、封禁 controller、握手/信令公钥不一致、workspace scope 拒绝、bridge `verifyPeerKey` 拒绝、offer 处理失败。
- `src/lib/teammate/teammateGroups.test.ts` 新增持久化字段防御解析、循环数据 fail-closed，以及真实 `TeammateGroupStore` 的工作区切换、localStorage 回读、编组变更、成员事件桥和任务记录路径。
- 既有接口与生产行为未改；本波仅增补测试契约，未向 Codex 外 CLI/agent 派发消息。

## 验证

- `pnpm exec vitest run src/lib/teammate/teammateGroups.test.ts packages/remote/src/shared/cloud/ridgeCloudProvider.test.ts`：45/45 通过。
- `pnpm test:coverage:sonar`：199 files；1835 passed / 1 skipped；statements `12539/18603 = 67.40%`；branches `6903/11608 = 59.46%`；functions `2448/3536 = 69.23%`；lines `11322/15891 = 71.24%`。
- 本地 statements 达到 80% 仍缺 `2344` 条；V8 对部分 `.mjs` 继续报告 `PARSE_ERROR/Expected ident` 并排除，故不宣称 Sonar 80% 或 Quality Gate 通过。

## 未闭环与交接边界

- Sonar project `>=80%`、Quality Gate、scanner/CE 成功证据仍未闭环；本地 LCOV 不替代 Sonar project metric。
- `HubPtyRuntimeSnapshot` 已完成原子 API/fencing 存储，但真实宿主五条件原子采样注入、第三方 Runtime/A2A 私有协议兼容仍缺现场/协议证据。
- 本波不纳入 `coverage/*`、`.iteration/*`、用户已有 CDP 改动及 NotebookLM 运行态；不 push、tag、release。
