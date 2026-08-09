# Agent 通信重构 / 覆盖波次 14 交接

日期：2026-08-09

## 已完成

- 按 Rust stable manifest 下载 `llvm-tools-1.97.1-x86_64-pc-windows-msvc.tar.gz`，SHA-256 `921e78752e1d205e315aff35eac32f15e5f3e64aaba6df7277a6c71b01882310` 校验通过；工具只放在本地 `.tools`。
- 修复 kernel 客户端有界请求超时：新增 `KERNEL_REQUEST_TIMEOUT_MS=5_000`，避免 instrumentation/PTY 创建负载下健康 endpoint 已就绪但 1.5s 读超时。
- 修复后使用 `RUSTFLAGS=-C instrument-coverage` 执行 `cargo test --workspace --all-targets --quiet`，`cargo_exit=0`，取得 551 个 `.profraw`；合并为 `.iteration/artifacts/rust-coverage-wave-15/rust.profdata`。
- `llvm-cov export` 生成项目源 198 条记录，并复制为 `coverage/rust.lcov`；`sonar-project.properties` 已加入 `sonar.rust.lcov.reportPaths=coverage/rust.lcov`。
- 当前 Rust 项目源 line coverage 为 `37,842/68,102 = 55.57%`，function coverage `4,081/7,943 = 51.38%`。此为真实本地报告，未伪造 80% 结果。

## 未闭合项

- wave14 曾记录 `packages/ridge-cli/tests/kernel_lifecycle_e2e.rs` 的 `kernel_pty_survives_client_detach_and_replays_after_cursor` 在 kernel PTY 建立处以 Windows `os error 10060` 失败；调高有界请求超时后，插桩单测 `1/1` 通过，wave15 workspace 全量 `cargo_exit=0`。日志：`.iteration/artifacts/rust-coverage-wave-15/cargo-test-workspace.log`、`.iteration/artifacts/rust-coverage-wave-14/kernel-lifecycle-rerun-after-timeout-fix.log`。
- Runtime API/A2A 仍只有严格 host probe/adapter contract；生产 host 没有未经验证的第三方 endpoint 实现。PTY 仍 fail-closed，缺少五条件同时成立的真实运行时证据。
- Sonar 未取得有效认证：`SONAR_TOKEN` 未注入，`admin/admin` API 重试失败，本机 Browser 无可用会话。未猜测、打印或保存 token，未伪造 scanner/CE/Quality Gate 结果。
- Sonar 项目级 coverage 仍不是 80%；当前 Rust 报告 line 55.57%、function 51.38%，前端 V8 本轮为 statements 52.42%、lines 55.59%。

## 证据与下一步

- Rust 原始导出：`.iteration/artifacts/rust-coverage-wave-15/rust.lcov`；项目过滤导出：`.iteration/artifacts/rust-coverage-wave-15/rust-project.lcov`；Sonar 输入：`coverage/rust.lcov`。
- Sonar 官方 Rust LCOV 属性：`sonar.rust.lcov.reportPaths`；需在有效认证下重新 scanner、等待 CE，再读取 project measures 与 Quality Gate。
- 先补覆盖缺口与 PTY E2E 连接根因，再做完整 scanner；工作区已有用户改动，勿 reset/checkout/stash/提交无关内容。

本波未向 Codex 之外 CLI 派发消息，未提交、推送或发布。
- Production host guard: Kernel and desktop now explicitly return MCP-pull-only delivery probes when no verified Runtime API/A2A endpoint or atomic five-field PTY snapshot exists. `ridge-kernel` 48/48 and desktop `ridge` 268/268 passed.
