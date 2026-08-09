# Sonar 波次 14 证据

- 配置：`sonar-project.properties` 同时引用 `coverage/lcov.info` 与 `coverage/rust.lcov`。
- Rust 工具：官方 Rust stable manifest 对应的 `llvm-tools-preview` Windows 归档，SHA-256 `921e78752e1d205e315aff35eac32f15e5f3e64aaba6df7277a6c71b01882310`。
- kernel 客户端新增有界 `KERNEL_REQUEST_TIMEOUT_MS=5_000`，修复 instrumentation/PTY 创建负载下 1.5s 读超时。
- 插桩：`cargo test --workspace --all-targets --quiet`，wave15 产出 551 个 profile 且 `cargo_exit=0`；合并文件 `.iteration/artifacts/rust-coverage-wave-15/rust.profdata`。
- 导出：`coverage/rust.lcov`，项目源 198 条 LCOV 记录，line `37,842/68,102 = 55.57%`，function `4,081/7,943 = 51.38%`。
- wave14 的 `kernel_lifecycle_e2e` PTY 建立连接超时（Windows `10060`）已由上述有界超时修复；wave15 workspace 全量通过。
- 本轮没有有效 Sonar 认证，故没有新的 scanner/CE/Quality Gate 结果；不以本地覆盖数字冒充 Sonar accepted metric。

结论：Rust LCOV 输入链已建立；Sonar 80% 与 Quality Gate 仍 ACTIVE。
