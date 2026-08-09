# Wave43 交接：PTY runtime snapshot 与脚本契约覆盖

日期：2026-08-10

## 已落地

- `packages/ridge-mcp/src/delivery.rs` 新增 `HubPtyRuntimeSnapshot`，将 PTY fallback 五条件与 `state_revision/input_epoch` 作为同一 host 观察提交；注册、刷新、过期、generation/lease teardown 均保持 fail-closed。
- `McpSessionState` 与 `packages/ridge-kernel/src/domain.rs` 已迁移到 runtime snapshot 注册/注销 API。第三方 Runtime/A2A 私有协议未冒充兼容。
- `scripts/build-ridge-mcp-sidecar.mjs`、`scripts/build-rdg-sidecar.mjs`、`scripts/check-release-version.mjs` 增加 ESM main guard 与可测导出；新增对应测试，直接执行路径保持不变。

## 验证

- `pnpm exec vitest run scripts/build-rdg-sidecar.test.mjs scripts/check-release-version.test.mjs scripts/build-ridge-mcp-sidecar.test.mjs`：`12/12`。
- `cargo test --target-dir target/codex-wave43-mcp -p ridge-mcp --features axum-transport --lib`：`93/93`。
- `cargo test --target-dir target/codex-wave43-kernel -p ridge-kernel --lib`：`49/49`。
- `pnpm test:coverage:sonar`：`199` files，`1825 passed / 1 skipped`；statements `12440/18603 = 66.87%`、branches `6869/11608 = 59.17%`、functions `2420/3536 = 68.43%`、lines `11239/15891 = 70.72%`，距本地 80% 缺 `2443`。

## 未闭环

- 本机 Sonar project 仍未达 `>=80%`，Quality Gate 仍 `ERROR`；本波无新 scanner/CE 成功证据。
- V8 对部分既有 `.mjs` 的 `PARSE_ERROR/Expected ident` 仍在；未改 coverage include/exclude。
- `HubPtyRuntimeSnapshot` API 已具备原子提交与 fencing，但真实宿主五条件采样注入、第三方 Runtime/A2A 真实协议兼容性仍需现场/协议证据。

## 工作区与交接纪律

- 未向 Codex 外 CLI/agent 派发消息；未 push、tag、release。
- `coverage/*`、`.iteration/*` 与用户既有 dirty 修改不纳入本波提交；提交时仅选择代码、测试与本交接文档。
- 下一步先复核 `pnpm check`、`cargo fmt --all -- --check`、`pnpm test`，再继续覆盖率热点与 Sonar scanner/CE 闭环。
