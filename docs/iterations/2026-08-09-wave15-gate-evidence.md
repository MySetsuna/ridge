# Wave15 闸门证据

- `requirements_gate.py assert-task-executable`：通过，`executable=true`，无 pending requirement。
- `iteration_gate.py`：使用仓库现存 `.iteration/context.json` 检查时返回 `write_scope_exceeded`；输出包含大量此前已存在的 dirty worktree 路径，且该 context 当前 JSON 解析也不稳定。
- 未用 `git reset`、`git checkout`、stash 或清理命令规避 dirty worktree；用户既有修改均保留。
- 代码质量回归：workspace Rust 插桩 `cargo_exit=0`；`ridge-mcp` 81/81；`ridge-kernel` 48/48；CLI lifecycle 3/3；Vitest 168 files / 1656 passed / 1 skipped；`svelte-check` 0 errors / 0 warnings；fmt 与 diff check 通过。
- 未闭合：Rust line 55.57%、前端 line 55.59%，尚未达到 Sonar full-project 80%；无有效 Sonar token，未产生新 accepted CE/Quality Gate；Runtime API/A2A 生产 endpoint 与 PTY 五条件运行证据仍缺。
- Ordinary workspace Rust regression: `cargo test --workspace --all-targets --quiet` exited 0; log: `.iteration/artifacts/cargo-test-workspace-wave-15b.log`.
- Latest regression: `ridge-kernel` 48/48, desktop `ridge` 268/268, workspace Rust 0 exit, Vitest 168 files / 1656 passed / 1 skipped, svelte-check 0/0, requirements gate executable with no pending IDs.
- Iteration gate remains non-green only because the stored context scope predates the large dirty worktree (`write_scope_exceeded`); no reset/checkout/stash/cleanup was performed.
- Latest isolated Rust coverage: `cargo test --workspace --all-targets --quiet --target-dir .iteration/artifacts/rust-target-wave-19` exited 0 with 1,552 profraw; `coverage/rust.lcov` reports 38,420/68,713 Rust project lines (55.91%) and 4,123/7,986 functions (51.63%).
- Wave17 Kernel teardown regression: `ridge-kernel` 49/49; PTY identity removal revokes registered Runtime API/A2A routes by generation/lease fence.
- Ordinary Wave17 workspace regression: 17 suites, 1,401 passed, 0 failed; warnings are existing dead-code/linker diagnostics, not test failures.
- Frontend Wave17b coverage: `coverage/lcov.info` contains 239 records, line 57.41% and function 55.68%; V8 parse-excluded existing scripts, therefore no 80% claim.
- Formal frontend regression: 173 files, 1,678 passed, 1 skipped; `pnpm check` 0 errors / 0 warnings.
- Wave17 does not close Sonar 80%; no authenticated scanner/CE/Quality Gate result was produced.
- Wave17b isolated LCOV run exits 0: 174 files / 1,682 passed / 1 skipped; line 57.41%, function 55.68%. `coverage/lcov.info` is refreshed; V8 parse exclusions remain for existing scripts, so Sonar 80% stays open.
- Wave18 headless adapter: `ridge-tmux` HTTP tests 13/13; desktop `ridge` tests 269/269; desktop `cargo check` passed. `NativeHttpCtx` now carries host-owned `McpSessionState`, and headless Runtime API/A2A dispatch uses the bounded fenced registry instead of global fallback.
- Wave18 persistence: `rdg` opens the shared `agent-hub.sqlite3` through Kernel registry before starting the headless MCP router; `cargo check -p ridge-cli` passed.
- Wave18 residuals: no verified cross-process Runtime/A2A endpoint, no production PTY five-field runtime proof, no authenticated Sonar 80%/Quality Gate result, and iteration gate remains `write_scope_exceeded` from pre-existing dirty scope.
