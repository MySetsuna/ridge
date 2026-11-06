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
