# 90 — 性能、回归与证据

要求见根[性能契约](../../specs/L2-PERF-001.md)及[Remote 交互可靠性](../../specs/l3-remote-interaction-reliability.md)。历史测试数量和 PASS 只属于当时构建。

## 验证入口

```sh
npm test
npm run lint
npm run typecheck
node scripts/check-architecture-docs.mjs
cargo test --locked -p ridge-cli --test cli_terminal_create_e2e
cargo test --locked -p ridge-kernel --test runtime_workspace_continuity
cargo test --locked -p ridge-kernel --test snapshot_history_runtime
cargo test --locked -p ridge-kernel --test terminal_lifecycle_runtime
```

命令见 [package.json](../../package.json)。真实 PTY 测试依赖平台、进程和 shell，忽略项及环境不足须单列。SpecTree 对当前锁执行 stc verify --run-tests，通过后 stc complete。

## 测量与故障

真实 child reply、输入到可见回显、队列、parse/feed/render 分别记录起止边界。报告构建、平台、传输、负载、重复次数、成功/超时数及 P50/P95/P99/max，再在可比条件下讨论变化。

[performance_baseline.rs](../../packages/ridge-kernel/tests/performance_baseline.rs)含需显式运行的基线。[remote_backpressure](../../packages/ridge-kernel/tests/remote_backpressure.rs)、[stability_fault](../../packages/ridge-kernel/tests/stability_fault.rs)及 [CLI 创建验收](../../packages/ridge-cli/tests/cli_terminal_create_e2e.rs)提供背压、故障与重试回归。

本轮文档检查核验链接、契约引用与历史边界。WAN、蜂窝、物理设备、跨平台及完整桌面渲染结论须引用实际执行报告。Foundation 基线见[历史性能导览](../archive/CHG-089/architecture/90-performance.md)及[验收记录](notes/README.md)。


## Commune 与近期 Remote 验收

[commune_registration](../../packages/ridge-kernel/tests/commune_registration.rs)验证原会话与原组绑定，[managed_agent](../../packages/ridge-kernel/tests/managed_agent.rs)验证显式协作启动和生命周期，[全局界面验收](../../scripts/commune-global-ui-acceptance.mjs)验证真实隔离 Runtime 与当前桌面/手机组件。受控协议进程可证明这些执行链路，不能代替真实外部 Agent 验收。

[已安装 CLI 初始化矩阵](../../scripts/commune-agent-interop-acceptance.mjs)使用独立 Runtime 数据目录，显式初始化真实 CLI 会话并退出，不发送模型提示。它不证明完整对话、工具审批或目标完成；外部 Agent 的服务与配置仍由其自身管理。获准跳过的工具须记录 SKIPPED，未验证能力不能计作通过。

[生产目录验收](../../scripts/remote-cwd-acceptance.mjs)覆盖真实 shell、LAN HTTPS/WSS 与隔离信令/账号下的 Cloud WebRTC/E2EE；[Cloud Host 验收](../../scripts/kernel-cloud-owner-acceptance.mjs)覆盖控制连接、授权与终端流。执行前构建相应 Kernel、CLI、WASM 和协议 fixture，按脚本及实际报告核对范围。浏览器窄屏、模拟器、物理手机、局域网和公网须分别记录。
