> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 26（约 2 日 · OP-GIT-BYPASS）

## 范围边界

Git 唯一出口硬化 + 观测 + 双端 cap；无 hosts 出站。

## 目标（6）

1. git_timeout_kill_count / git_acquire_timeout_count。
2. timeout/acquire 路径递增。
3. production_git_spawn_only_via_git_cmd 静态门禁。
4. hang 子进程超时杀树测断言计数。
5. GitGuardStats + get_git_guard_stats 桌面命令。
6. pLimit GIT_CONCURRENCY_MIN/MAX 与 rust 交叉 vitest。

## 验收

| # | 信号 |
| --- | --- |
| 1 | guard_tests 全绿 |
| 2 | get_git_guard_stats 注册于 lib.rs |
| 3 | pLimit.test.ts 交叉常量绿 |

## 代码面

`ridge-core/.../git.rs`、`src-tauri/commands/git.rs` wrapper、`pLimit.ts`+test。

