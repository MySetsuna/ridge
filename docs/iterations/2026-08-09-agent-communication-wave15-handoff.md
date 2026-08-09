# Agent 通信架构重构 · Wave15 交接

日期：2026-08-09

## 范围与来源

- 主 Notebook：`Ridge 项目现状、愿景与规划基线（2026-07-21）`。
- 主 source：`9516749e-c317-4f13-9cda-b64b00cec465`；关联 note：`66919cb9-1329-4ddf-955c-f426d15a9fe6`。
- 临时对话 source：`df4d5dcc-9813-4c61-ae9f-1e9199cb7555`；关联 note：`f6ffd900-708d-44ee-9818-1a3269c533fc`。
- 需求总表：`docs/REQUIREMENTS-SPEC.md`；详细归纳：`docs/iterations/2026-08-09-agent-communication-architecture-requirement.md`。

## 本轮已落地

1. Kernel/Teammate identity、lifecycle、generation/lease fencing、typed envelope、SQLite Hub、MCP Hub tools、delivery priority 与 receipt 语义已有代码和确定性测试。
2. Kernel 与桌面 host 显式实现 `probe_delivery`：当前仅宣称 `mcp_pull`；未验证 Runtime API/A2A endpoint，且无 PTY 五字段原子运行时快照时，不宣称更高能力，不绕 Hub 直接写 PTY。
3. Kernel 请求读写超时收敛为命名常量 `KERNEL_REQUEST_TIMEOUT_MS=5_000`，避免 PTY/插桩负载下健康 endpoint 因 1.5 秒硬超时产生假失败。
4. Rust 插桩全 workspace：`cargo_exit=0`，551 个 profile；Rust 项目源 line `37,842/68,102=55.57%`，function `4,081/7,943=51.38%`。
5. 普通 Rust 全 workspace：`cargo test --workspace --all-targets --quiet` exit 0。
6. 前端回归：正式 `test:coverage:sonar` 173 files / 1678 passed / 1 skipped；svelte-check 0 errors / 0 warnings；LCOV line `8,996/15,807=56.91%`、branch `5,614/11,511=48.77%`。
7. 质量检查：`cargo fmt --all -- --check`、`git diff --check`、requirements gate 均通过；requirements gate 为 executable、无 pending IDs。

## 尚未闭合，不能宣称完成

- Runtime API/A2A：仓库未发现可验证的第三方或 Agent Runtime endpoint/protocol，生产 host 仍保持 MCP pull-only。后续须先取得真实 endpoint、capability probe、认证边界、超时/取消和端到端 receipt，再接入 adapter；禁止凭 Agent 名称或 capability 字符串硬编码。
- PTY fallback：生产状态尚未同时提供 `agent_idle`、`terminal.mode=agent_prompt`、`pending_approval=false`、前台进程归属、无用户输入竞争五项原子证据，故 fallback 继续 fail-closed。
- Sonar：本地真实 Rust/前端覆盖仍约 55%，未达到全项目 80%。已接受历史全扫描为 coverage 40.3%、line 41.2%、branch 38.9%、Quality Gate ERROR；本轮没有有效 Sonar token，不能伪造新的 scanner/CE/Gate 结果。Sonar Rust LCOV 属性为 `sonar.rust.lcov.reportPaths=coverage/rust.lcov`。
- iteration gate：当前 `.iteration/context.json` 的旧 write scope 与仓库既有大规模 dirty worktree 冲突，返回 `write_scope_exceeded`；未执行 reset、checkout、stash 或清理用户修改。

## 关键证据

- Rust workspace：`.iteration/artifacts/cargo-test-workspace-wave-15b.log`
- Rust 插桩：`.iteration/artifacts/rust-coverage-wave-15/cargo-test-workspace.log`
- Rust LCOV：`coverage/rust.lcov`
- Kernel：`.iteration/artifacts/ridge-kernel-regression-wave-15c.log`
- 桌面：`.iteration/artifacts/ridge-desktop-regression-wave-15c.log`
- Vitest：`.iteration/artifacts/vitest-regression-wave-15c.log`
- svelte-check：`.iteration/artifacts/svelte-check-wave-15c.log`
- requirements gate：`.iteration/artifacts/requirements-gate-wave-15c.json`
- iteration gate：`.iteration/artifacts/iteration-gate-wave-15c.log`
- Sonar 认证失败：`.iteration/artifacts/sonar-scan-wave-13.log`

## 复现命令

```powershell
cargo test --workspace --all-targets --quiet
cargo test -p ridge-kernel --lib --quiet
cargo test -p ridge --lib --quiet
node C:\DevKit\nvm\v20.19.0\node_modules\pnpm\bin\pnpm.cjs check
node C:\DevKit\nvm\v20.19.0\node_modules\pnpm\bin\pnpm.cjs exec vitest run --no-file-parallelism
```

本轮未向 Codex 之外的 CLI 派发消息，未提交、推送、发布或修改外部服务数据。
- Wave16 continuation: `DeliveryRegistry` now provides bounded host-owned Runtime API/A2A subscriptions with `try_send`, current generation/lease fencing, explicit full/disconnected errors; Kernel and desktop hosts delegate to it.

## Wave17 continuation

- Correctly injected `RUSTFLAGS=-C instrument-coverage`; `cargo test --workspace --all-targets --quiet` exit 0, 551 profraw.
- Rebuilt `coverage/rust.lcov` from an isolated full target object set after latest code: 199 project records, Rust line `38,420/68,713=55.91%`, function `4,123/7,986=51.63%`; this is evidence input only, not Sonar 80% acceptance.
- Kernel PTY identity teardown now unregisters both Runtime API and A2A routes with the removed identity's generation/lease; regression proves a destroyed Agent is no longer probed as Runtime-capable.
- `ridge-kernel` regression: 49/49; `cargo fmt --all -- --check`: pass.
- Ordinary workspace regression after teardown wiring: 17 suites, 1,401 passed, 0 failed; compiler emitted existing dead-code/linker warnings only.
- Frontend coverage regeneration: formal `test:coverage:sonar` 173 files / 1,678 passed / 1 skipped; LCOV line `8,996/15,807=56.91%`, branch `5,614/11,511=48.77%`; V8 still reports parse exclusions for existing non-test scripts, so Sonar 80% remains open.
- Added deterministic tests for link/path resolution, project search/replace, settings persistence, and pane docking; focused tests all green.
- Isolated Rust evidence: `.iteration/artifacts/rust-coverage-wave-19/cargo-test-workspace.log` and `.iteration/artifacts/rust-coverage-wave-19-raw/`.
- Residual unchanged: no cross-system real Runtime/A2A endpoint, no PTY five-field atomic runtime proof, no authenticated Sonar CE/Quality Gate result, and iteration gate still reports stored-scope `write_scope_exceeded`.

### Wave17b continuation

- Added `cloudHostStore` lifecycle tests; full Vitest regression now passes 173 files / 1,679 tests / 1 skipped.
- Isolated LCOV run (separate report directory, single `lcov` reporter) exits 0: 174 files / 1,682 passed / 1 skipped; line `9,075/15,807=57.41%`, function `1,964/3,527=55.68%`.
- `coverage/lcov.info` refreshed from the isolated report. V8 still emits parse exclusions for existing `.mjs` scripts; no 80% claim.

### Wave18 continuation

- `ridge-tmux`/headless `rdg` now receives an explicit host-owned `McpSessionState` instead of using the process-global default Hub; desktop injects its persistent Hub state into the shared native context.
- `rdg tmux` opens the same persistent `agent-hub.sqlite3` via Kernel registry before injecting that state, so headless Hub receipts survive process lifetime.
- Headless `TmuxMcpHost` now exposes the same Runtime API/A2A probe and bounded dispatch seam as Kernel/Desktop, with registry generation/lease fencing preserved.
- Proof: isolated `ridge-tmux` HTTP tests 13/13; isolated desktop `ridge` tests 269/269; desktop `cargo check` passed. Artifacts: `.iteration/artifacts/ridge-tmux-http-wave18-isolated.log`, `.iteration/artifacts/ridge-check-wave18-isolated.log`, `.iteration/artifacts/ridge-lib-wave18-isolated.log`.
- Headless CLI `cargo check` passed; artifact `.iteration/artifacts/rdg-check-wave18-isolated.log`.
- Residuals remain unchanged: no verified cross-process Runtime/A2A endpoint, no atomic production PTY five-field proof, Sonar full-project 80%/Quality Gate open, and stored-scope iteration gate still reports `write_scope_exceeded`.

### Wave20 continuation

- `DeliveryRegistry` now permits a newer generation to atomically replace an older same-Agent route before old teardown completes. The old receiver disconnects; stale registration, delivery, probe, and teardown cannot affect the new generation.
- `cargo test -p ridge-mcp --lib --quiet --target-dir .iteration/artifacts/rust-target-wave-20-mcp`: 85/85 passed. Artifact: `.iteration/artifacts/ridge-mcp-reconnect-fence-wave20.log`.
- This closes the local reconnect-fence race only; cross-process Runtime/A2A, PTY five-field runtime proof, Sonar 80%/Gate, and iteration write-scope remain open.
- Formal frontend coverage refresh: 174 files / 1,684 passed / 1 skipped; statements `54.21%`, branches `49.10%`, functions `55.88%`, lines `57.47%`. Artifact: `.iteration/artifacts/vitest-coverage-wave20.log`.
- `cargo fmt --all -- --check`, `git diff --check`, and requirements gate pass. Direct `svelte-check` and `ridge-cli` test attempts exceeded the tool wall-clock limit during first compilation; no relevant child process remained afterward, so they are recorded as timeout, not pass.
