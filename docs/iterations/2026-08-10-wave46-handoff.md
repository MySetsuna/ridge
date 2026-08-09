# Wave46 交接：通信 topology、终端背压与 pane 投影覆盖

日期：2026-08-10

## 本波落地

- `src/lib/remote/cloud/cloudHostTopologyLink.test.ts` 补齐多 workspace 查询、shell 查询、切换/重命名/保存、断连释放、空 pane、Agent 注册/注销与 pane-scoped shell 激活分支。
- `packages/remote/src/shared/terminal/manager.test.ts` 补齐 inline-TUI ESC 分片、非 inline 切换、bounded feed deferred 队列、老 wasm fallback、输入 ownership、Mac Cmd、wheel 上限、空协议响应与未知 pane 边界。
- `src/lib/stores/paneTree.coverage.test.ts` 补齐 Agent attention/status、pane/CWD 投影、ratio anchor、junction registry、desktop reattach gate、CWD listener replacement/回调与 stale path fallback。
- `packages/remote/src/shared/cloud/controllerCloudProvider.test.ts` 补齐 ICE 获取失败、WebSocket 构造失败、畸形信令、ICE candidate 失败、可恢复 error、peer leave 与 answer 应答失败。

本波仅增确定性测试与证据；未改生产运行语义，未向 Codex 外 CLI/Agent 发消息。

## 验证

- topology 聚焦：`15/15`。
- 终端聚焦：`20/20`。
- paneTree 聚焦：`10/10`。
- 全量：`pnpm test`，`199` files，`1844 passed / 1 skipped`。
- 质量门：`pnpm check`，`0 errors / 0 warnings`（Wave47 修正新增 fixture 类型后复核）。
- 本地 V8/LCOV（Wave47 重跑）：statements `12664/18603 = 68.07%`；branches `6982/11608 = 60.14%`；functions `2475/3536 = 69.99%`；lines `11423/15891 = 71.88%`。
- 本地 statements 80% 目标需 `14883` 条，当前尚缺 `2219` 条。
- Wave47 controller 聚焦：`26/26`。

## 未闭环

- Sonar project coverage `>=80%`、Quality Gate OK、scanner/CE 成功证据仍未闭环；本地 LCOV 不替代 Sonar 项目指标。
- `.mjs` coverage 仍有 Rollup `PARSE_ERROR/Expected ident`，需后续修复可测化/解析边界，不以排除代码冒充覆盖率。
- PTY 五条件原子运行时采样、第三方 Runtime/A2A 私有协议兼容仍需现场或等价证据。
- `coverage/*`、`.iteration/*`、NotebookLM 运行态及用户既有 CDP 修改继续保留 dirty，不纳入提交。

## 下一步

优先继续真实缺口：`packages/remote/src/shared/terminal/manager.ts`、`packages/remote/src/shared/transport/wsRemote.ts`、Cloud provider 与脚本可测化；每波复跑全量测试、`pnpm check`、Sonar scanner/CE（若环境可用），再更新本交接链。
