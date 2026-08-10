# Wave 82：Cloud Host pane 流接线核验（2026-08-11）

## 结论

Cloud Host 的真实 PTY 流并非未接入：生产入口 `src/lib/remote/cloud/cloudHostStore.ts` 在每次 `createBridge` 中构造 `makeCloudHostPaneSource({ invoke, listen })`，并将其注入 `CloudHostBridge.paneOutputSource`；共享工作区路径还保留 pane scope 校验。

## 变更

- `src/lib/remote/cloud/cloudHostStore.test.ts` 新增断言：普通 host 与 scoped host 均注入 pane source，source 工厂按 bridge 创建次数调用。
- `packages/remote/src/shared/cloud/cloudHostBridge.ts` 将无 source 的兼容回退日志从 `TODO` 改为明确的 `live stream unavailable`，避免把测试/兼容构造误报为生产接线缺失。
- 对应 bridge 测试名称同步澄清；运行语义不变。

## 验证

- Cloud focused：3 files / 78 passed / 0 failed。
- 全量测试、`pnpm check`、构建与 PWA 验证沿用 Wave80/Wave81 证据。
- 生产接线仍需真实 Cloud/WebRTC/设备现场验收；本轮不把 mock source 测试冒充公网链路证据。
