> **SUPERSEDED for AC4 accounting** — 原拆账不计 10 分。诚实 credit 见 `CONTRACT-iteration-40`…`49` 与 `{SCRATCH}/ac4-ledger.md`。
# CONTRACT — Iteration 31（约 2 日 · 外延进程树护栏 + foreign 关视图 + git 遥测 UI）

## 范围边界（必须代码，禁止文档充数）

本轮**独占**交付（不与 22–30 重复计）：

1. `ridge-core::process_guard`：共享 wall-clock timeout + 进程树杀（git 改调之）。
2. `kill_pty_if_present` foreign 分支：关视图 = detach_foreign，不写 exit。
3. `paneTree.closePane` 前端对 remote/rdg origin 先 `detach_host_session`。
4. `gitGuardStats` store + AgentCenter 护栏 badge。
5. `get_git_guard_stats` IPC + process_guard 测。
6. user-rail / desktop-only 脚本回归。

## 目标（7）

1. process_guard::run_command_with_timeout 超时杀树 + 计数。
2. process_guard 单测：真挂起假二进制 + 快速命令成功。
3. git kill_pid_tree 委托 process_guard（唯一 kill 实现）。
4. foreign close 路径不杀远端 PTY。
5. closePane 前端 origin 检测 + detach。
6. gitGuardNeedsAttention + AgentCenter 展示。
7. 门禁：process_guard + guard_tests + hosts + vitest gitGuardStats。

## 验收

| # | 信号 |
| --- | --- |
| 1 | `cargo test -p ridge-core --lib process_guard` 绿 → `gates-iter-31.log` |
| 2 | `cargo test -p ridge-core --lib guard_tests` 绿 |
| 3 | vitest `gitGuardStats.test.ts` 绿 |
| 4 | `kill_pty_if_present` 含 is_foreign + detach_foreign |

## 停机

空 Release；仅改 markdown 算完成。

