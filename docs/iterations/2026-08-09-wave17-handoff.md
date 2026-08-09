# Wave17 交接：Agent 通信架构与质量闸

日期：2026-08-09

## 本轮落地

- `ridge-mcp` 新增 host-owned bounded Runtime API/A2A delivery registry：`try_send`、256 队列上限、generation/lease fencing、full/disconnected/stale 错误。
- Kernel 与桌面 MCP host 接入 Runtime/A2A probe 与 dispatch；Kernel PTY identity 销毁路径撤销同一 Agent 的 Runtime/A2A route，防止已销毁 identity 继续被探测或投递。
- 新增 cloud host 单例生命周期测试；覆盖缺凭据、桌面 daemon 启停/失败、浏览器 host 显式上线下线、controller kick/blacklist。
- 保留 fail-closed：未发现可验证的跨系统 Runtime/A2A endpoint，不硬编码第三方协议；PTY 五条件未同时具备时不走 fallback。

## 验证结果

```text
cargo test --workspace --all-targets --quiet     17 suites / 1,401 passed / 0 failed
cargo test -p ridge-kernel --lib --quiet         49 passed / 0 failed
pnpm check                                       0 errors / 0 warnings
vitest run --no-file-parallelism                 173 files / 1,679 passed / 1 skipped
requirements_gate assert-task-executable        executable=true / pending=0
git diff --check                                 exit 0
cargo fmt --all -- --check                       exit 0
```

Rust 插桩 Wave17：551 profraw；`coverage/rust.lcov` 198 records，line `38,351/70,832=54.14%`，function `4,119/8,278=49.76%`。

前端隔离 LCOV：174 files / 1,682 passed / 1 skipped；`coverage/lcov.info` line `9,075/15,807=57.41%`，function `1,964/3,527=55.68%`。V8 对现有未测试 `.mjs` 脚本仍报 parse exclusion；这不是 Sonar 80% 证据。

## 尚未闭环

1. Sonar 真实全项目 coverage 仍未达 80%；本机 scanner 最新认证失败为 HTTP 401，未产生新的可信 CE/Quality Gate。历史 accepted full scan 为 coverage 40.3%、line 41.2%、branch 38.9%、Gate ERROR。
2. 尚无跨进程/跨系统真实 Runtime API 或 A2A endpoint、认证、取消、超时和 receipt E2E；当前 registry 仅是 host-owned in-process seam。
3. 生产 PTY 尚无五条件原子快照：`agent_idle`、`terminal.mode=agent_prompt`、`pending_approval=false`、前台进程归属、无用户输入竞争。
4. `iteration_gate.py` 仍因既有大规模 dirty worktree 返回 `write_scope_exceeded`；未 reset、checkout、stash、清理或改动用户既有文件以绕过。

## 复现

```powershell
cargo test --workspace --all-targets --quiet
cargo test -p ridge-kernel --lib --quiet
node C:\DevKit\nvm\v20.19.0\node_modules\pnpm\bin\pnpm.cjs check
node C:\DevKit\nvm\v20.19.0\node_modules\pnpm\bin\pnpm.cjs exec vitest run --no-file-parallelism
node C:\DevKit\nvm\v20.19.0\node_modules\pnpm\bin\pnpm.cjs exec vitest run --coverage --coverage.processingConcurrency=1 --coverage.reportsDirectory=.iteration/artifacts/vitest-coverage-wave17 --coverage.reporter=lcov --no-file-parallelism
```

## Wave18 continuation

- Headless `ridge-tmux`/`rdg` now receives host-owned `Arc<McpSessionState>` through `NativeHttpCtx`; its MCP router no longer silently falls back to the process-global Hub.
- `TmuxMcpHost` now delegates Runtime API/A2A probe and dispatch to the same bounded registry, including generation/lease fencing and non-blocking delivery; desktop native context injects the persistent desktop Hub state.
- `rdg tmux` now opens the same persistent `agent-hub.sqlite3` through `ridge_kernel::registry::agent_hub_path()` before injecting the state, instead of using an ephemeral default.
- Deterministic headless route test: `cargo test -p ridge-tmux --features http --lib --quiet --target-dir .iteration/artifacts/rust-target-wave-18-tmux` — 13 passed / 0 failed; artifact `.iteration/artifacts/ridge-tmux-http-wave18-isolated.log`.
- Desktop compile/test: `cargo check -p ridge --quiet --target-dir .iteration/artifacts/rust-target-wave-18-ridge` exit 0; `cargo test -p ridge --lib --quiet --target-dir .iteration/artifacts/rust-target-wave-18-ridge` — 269 passed / 0 failed; artifacts `.iteration/artifacts/ridge-check-wave18-isolated.log` and `.iteration/artifacts/ridge-lib-wave18-isolated.log`.
- Headless CLI compile: `cargo check -p ridge-cli --quiet --target-dir .iteration/artifacts/rust-target-wave-18-rdg` exit 0; artifact `.iteration/artifacts/rdg-check-wave18-isolated.log`.
- This closes the headless host adapter seam only. It does not prove a cross-process third-party Runtime/A2A endpoint, the PTY five-field runtime snapshot, Sonar project coverage >=80%/Quality Gate, or a clean iteration gate.
- No message was sent to any CLI or agent outside Codex; no commit, push, release, or external service mutation was performed.

证据目录：`.iteration/artifacts/rust-coverage-wave-17/`、`.iteration/artifacts/vitest-coverage-wave17/`、`.iteration/artifacts/cargo-test-workspace-wave17-teardown.log`、`.iteration/artifacts/vitest-wave17b.log`、`.iteration/artifacts/requirements-gate-wave17c.json`。

本轮未向 Codex 之外的 CLI 派发消息；未提交、推送、发布或修改外部服务数据。
